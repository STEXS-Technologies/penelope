//! Application-owned verification policy composed with a real journal and worker.
//! The verifier is a trusted test fixture, not a cryptographic proof implementation.
use super::*;
use penelope_statechronicle::task::{
    CheckpointStateError, CheckpointStateEvidence, CheckpointStateVerifier, verify_checkpoint_state,
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};

struct Policy {
    scope: TaskScope,
    checkpoint: BlobRef,
    mode: AtomicU8,
    calls: AtomicUsize,
}
impl CheckpointStateVerifier for Policy {
    fn verify(&self, record: &TaskRecord) -> Result<CheckpointStateEvidence, CheckpointStateError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if record.scope != self.scope
            || record.event
                != (TaskExecutionEvent::CheckpointCommitted {
                    checkpoint: self.checkpoint,
                })
        {
            return Err(CheckpointStateError::BindingMismatch);
        }
        let mode = self.mode.load(Ordering::SeqCst);
        if mode == 1 {
            return Err(CheckpointStateError::VerificationFailed);
        }
        let mut evidence = CheckpointStateEvidence {
            scope: self.scope,
            generation: record.generation,
            checkpoint: self.checkpoint,
            state_version: 7,
            state_digest: Digest([8; 32]),
            proof_digest: Digest([9; 32]),
        };
        match mode {
            2 => evidence.generation = TaskGeneration::ZERO,
            3 => evidence.checkpoint.digest = Digest([0; 32]),
            4 => evidence.scope.input = TaskInputDigest::from_digest(Digest([0; 32])),
            _ => {}
        }
        Ok(evidence)
    }
}
struct VerifiedJournal {
    inner: SqliteTaskJournal,
    policy: Arc<Policy>,
}
impl TaskJournal for VerifiedJournal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        self.inner.read(scope, sequence)
    }
    fn read_blob(
        &mut self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        self.inner.read_blob(scope, blob, buffer)
    }
    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        if matches!(
            record.event,
            TaskExecutionEvent::CheckpointCommitted { .. }
                | TaskExecutionEvent::CheckpointMigrated { .. }
        ) {
            verify_checkpoint_state(record, self.policy.as_ref()).map_err(|error| match error {
                CheckpointStateError::VerificationFailed => TaskPortError::Unavailable,
                CheckpointStateError::NotCheckpoint | CheckpointStateError::BindingMismatch => {
                    TaskPortError::Corrupt
                }
            })?;
        }
        self.inner.commit(record, blob)
    }
}
type GatedSupervisor = TaskSupervisor<VerifiedJournal, Runtime, 64>;
fn observe(supervisor: &mut GatedSupervisor) -> Result<bool, SupervisorError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = supervisor.poll(TaskTick::new(2));
        if result != Ok(false) {
            return result;
        }
        if Instant::now() >= deadline {
            return Err(TaskPortError::Runtime.into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn independent_checkpoint_gate_controls_real_worker_ack_and_reconstruction() {
    for mode in 0..5 {
        let directory = tempfile::tempdir().unwrap();
        let input = [5, 4, 5];
        let definition = definition(2);
        let scope = scope(definition, &input);
        let policy = Arc::new(Policy {
            scope,
            checkpoint: BlobRef {
                digest: Digest::of(&9u64.to_le_bytes()),
                bytes: 8,
                schema: 1,
            },
            mode: AtomicU8::new(mode),
            calls: AtomicUsize::new(0),
        });
        let path = directory.path().join("tasks.db");
        let journal = VerifiedJournal {
            inner: SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap(),
            policy: Arc::clone(&policy),
        };
        let authority =
            SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
        let runtime = Runtime::new(authority, config(directory.path())).unwrap();
        let mut supervisor =
            GatedSupervisor::restore(journal, runtime, scope, definition, TaskTick::ZERO).unwrap();
        supervisor.submit(&input, 1, TaskTick::ZERO).unwrap();
        supervisor.advance(TaskTick::new(1)).unwrap();
        let result = observe(&mut supervisor);
        assert_eq!(policy.calls.load(Ordering::SeqCst), 1);
        if mode == 0 {
            assert_eq!(result, Ok(true));
            assert_eq!(
                supervisor.projection().checkpoint(),
                Some(policy.checkpoint)
            );
            observe(&mut supervisor).unwrap();
            assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
            continue;
        }
        assert_eq!(
            result,
            Err(if mode == 1 {
                TaskPortError::Unavailable
            } else {
                TaskPortError::Corrupt
            }
            .into())
        );
        assert_eq!(supervisor.projection().checkpoint(), None);
        assert_eq!(
            supervisor.advance(TaskTick::new(3)),
            Err(SupervisorError::ReconstructionRequired)
        );
        let generation = supervisor.projection().generation();
        let (mut rejected_journal, mut paused_runtime) = supervisor.into_parts();
        assert_eq!(
            rejected_journal.read(scope, TaskSequence::new(2)).unwrap(),
            None
        );
        // The worker cannot emit its completion until the exact checkpoint ACK.
        assert_eq!(
            paused_runtime
                .poll(scope, generation, &mut [0; 64])
                .unwrap()
                .progress,
            RuntimeProgress::Pending
        );
        let recovered = GatedSupervisor::restore(
            rejected_journal,
            paused_runtime,
            scope,
            definition,
            TaskTick::new(3),
        )
        .unwrap();
        assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
        assert_eq!(recovered.projection().checkpoint(), None);
        let (mut recovered_journal, mut fenced_runtime) = recovered.into_parts();
        assert!(matches!(
            recovered_journal
                .read(scope, TaskSequence::new(2))
                .unwrap()
                .unwrap()
                .event,
            TaskExecutionEvent::FaultObserved {
                fault: TaskFault::SupervisorInterrupted
            }
        ));
        fenced_runtime.fence(scope, generation).unwrap();
        assert_eq!(policy.calls.load(Ordering::SeqCst), 1);
    }
}
