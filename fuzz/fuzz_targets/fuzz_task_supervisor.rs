#![no_main]
// Reuse test-only computation and fault-injection adapters. None are shipped
// with the library or required by application runtime implementations.
#[path = "../../crates/penelope-executor/tests/support/tasks.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use penelope_ports::task::{runtime::*, *};
use std::{cell::RefCell, rc::Rc};
use support::{Cut, MemoryJournal, SumRuntime, definition, scope};

#[derive(Clone, Default)]
struct Journal(Rc<RefCell<MemoryJournal>>);
impl TaskJournal for Journal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        self.0.borrow_mut().read(scope, sequence)
    }
    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        self.0.borrow_mut().commit(record, blob)
    }
    fn read_blob(
        &mut self,
        scope: TaskScope,
        blob: BlobRef,
        dst: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        self.0.borrow_mut().read_blob(scope, blob, dst)
    }
}

type Supervisor = TaskSupervisor<Journal, SumRuntime, 64>;

fuzz_target!(|data: &[u8]| {
    let first = data.first().copied().unwrap_or(0);
    let mut definition = definition(
        match first & 3 {
            0 => RecoveryMode::RestartFromBeginning,
            1 => RecoveryMode::RestartFromCheckpoint,
            2 => RecoveryMode::ReconcileBeforeRestart,
            _ => RecoveryMode::NeverRestart,
        },
        if first & 4 == 0 {
            EffectContract::Pure
        } else {
            EffectContract::ReconcileRequired
        },
    );
    definition.recovery.max_attempts = u32::from(first % 8) + 1;
    definition.recovery.timeout_ticks = u64::from(first % 15) + 1;
    definition.budget.max_history_records = u32::from(first % 32).max(4);
    definition.budget.max_checkpoint_commits = u32::from(first % 8);
    definition.budget.deadline_tick = TaskTick::new(u64::from(first) + 10);
    let input = [1; 64];
    let scope = scope(definition, &input);
    let journal = Journal::default();
    let mut supervisor = Supervisor::restore(
        journal.clone(),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    for (index, chunk) in data.chunks(4).take(64).enumerate() {
        let byte = |position: usize| chunk.get(position).copied().unwrap_or(0);
        let tick = TaskTick::new(index as u64 + 1);
        journal.0.borrow_mut().cut = Some(match byte(1) % 5 {
            0 => Cut::BeforeCommit,
            1 => Cut::AfterCommit,
            2 => Cut::WrongReceipt,
            3 => Cut::StorageFull,
            _ => Cut::None,
        });
        let before = *supervisor.projection();
        let observation_scope = if byte(2) & 1 == 0 {
            scope
        } else {
            TaskScope {
                task: TaskId::from_digest(Digest([9; 32])),
                ..scope
            }
        };
        let generation = if byte(2) & 2 == 0 {
            before.generation()
        } else {
            before.generation().saturating_add(1)
        };
        let bytes = if byte(3) == u8::MAX {
            u32::MAX
        } else {
            u32::from(byte(3))
        };
        let output = [7; 64];
        let result = match byte(0) % 8 {
            0 => supervisor.submit(&input, 1, tick),
            1 => supervisor.advance(tick).map(|_| ()),
            2 => supervisor.poll(tick).map(|_| ()),
            3..=5 => supervisor
                .observe(
                    TaskObservation {
                        scope: observation_scope,
                        generation,
                        progress: match byte(0) % 8 {
                            3 => RuntimeProgress::Checkpoint { bytes, schema: 1 },
                            4 => RuntimeProgress::Complete { bytes, schema: 1 },
                            _ => RuntimeProgress::Fault(TaskFault::Crash),
                        },
                    },
                    &output[..usize::from(byte(2) % 65)],
                    tick,
                )
                .map(|_| ()),
            6 => supervisor.reconcile(
                observation_scope,
                generation,
                if byte(3) & 1 == 0 {
                    Reconciliation::Unknown {
                        evidence: Digest([8; 32]),
                    }
                } else {
                    Reconciliation::RestartAuthorized {
                        evidence: Digest([8; 32]),
                    }
                },
                None,
                tick,
            ),
            _ => {
                let (journal, runtime) = supervisor.into_parts();
                supervisor = match Supervisor::restore(journal, runtime, scope, definition, tick) {
                    Ok(restored) => restored,
                    Err(_) => break, // Failed reconstruction never authorizes execution.
                };
                Ok(())
            }
        };
        // Every acknowledged operation must agree with authoritative history.
        // Lost receipts may leave the in-memory projection behind, never ahead.
        let committed = replay(scope, definition, journal.0.borrow().records.clone()).unwrap();
        assert!(supervisor.projection().next_sequence() <= committed.next_sequence());
        if result.is_ok() {
            assert_eq!(*supervisor.projection(), committed);
        }
        if matches!(result, Err(SupervisorError::Port(_))) {
            assert_eq!(
                supervisor.advance(tick),
                Err(SupervisorError::ReconstructionRequired)
            );
        }
        // Size admission is independent of how many bytes the adapter managed
        // to supply. Matching live oversized reports must fence and poison.
        let size_limit = if byte(0) % 8 == 3 {
            definition.recovery.max_checkpoint_bytes
        } else {
            definition.budget.max_output_bytes
        };
        if matches!(byte(0) % 8, 3 | 4)
            && before.status() == TaskStatus::Running
            && observation_scope == scope
            && generation == before.generation()
            && tick < before.execution_deadline()
            && bytes > size_limit
            && result != Err(SupervisorError::ReconstructionRequired)
        {
            assert_eq!(
                result,
                Err(SupervisorError::Decision(TaskError::QuotaExhausted))
            );
            assert_eq!(*supervisor.projection(), before);
            assert_eq!(
                supervisor.advance(tick),
                Err(SupervisorError::ReconstructionRequired)
            );
        }
        if matches!(
            result,
            Err(SupervisorError::Decision(
                TaskError::ScopeMismatch | TaskError::StaleGeneration | TaskError::InvalidOrder
            ))
        ) {
            assert_eq!(*supervisor.projection(), before);
        }
        if let Some(output) = supervisor.projection().output() {
            assert_eq!(committed.status(), TaskStatus::Completed);
            let state = journal.0.borrow();
            output
                .verify(state.blobs.get(&output.digest.0).unwrap(), output.schema)
                .unwrap();
        }
        assert!(committed.generation().get() <= definition.recovery.max_attempts);
        assert!(committed.next_sequence().get() <= definition.budget.max_history_records);
    }
});
