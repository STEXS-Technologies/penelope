#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
use penelope_core::task::{runtime::*, *};
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[path = "support/runtime_fixture.rs"]
mod runtime_fixture;
use runtime_fixture::{Runtime, Supervisor, config, definition, fixture};

fn scope(definition: TaskDefinition, input: &[u8]) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(input)),
    }
}
fn fresh(directory: &Path, input: &[u8], cpu: u64) -> Supervisor {
    let definition = definition(cpu);
    let path = directory.join("tasks.db");
    let journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let runtime = Runtime::new(authority, config(directory)).unwrap();
    let mut supervisor = Supervisor::restore(
        journal,
        runtime,
        scope(definition, input),
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    supervisor.submit(input, 1, TaskTick::ZERO).unwrap();
    supervisor
}
fn wait(supervisor: &mut Supervisor) -> Result<bool, SupervisorError> {
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
fn static_worker_completes_only_after_durable_input_and_start() {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[0, 1, 2, 3], 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    wait(&mut supervisor).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
    let output = supervisor.projection().output().unwrap();
    let scope = supervisor.projection().scope();
    let (mut journal, _runtime) = supervisor.into_parts();
    let mut bytes = [0; 8];
    journal.read_blob(scope, output, &mut bytes).unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 6);
}
#[test]
fn kernel_blocks_process_filesystem_network_limit_and_descriptor_escape_syscalls() {
    // fork, openat, socket, setrlimit, clone, kill, ptrace, mutating
    // prlimit64, read outside stdin, write outside stdout/stderr, execve,
    // execveat through another descriptor/path, and descriptor rebinding.
    for mode in [2, 3, 4, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21] {
        let directory = tempfile::tempdir().unwrap();
        let mut supervisor = fresh(directory.path(), &[mode], 2);
        supervisor.advance(TaskTick::new(1)).unwrap();
        wait(&mut supervisor).unwrap();
        assert_eq!(supervisor.projection().status(), TaskStatus::Faulted);
        let scope = supervisor.projection().scope();
        let sequence = supervisor.projection().next_sequence().saturating_sub(1);
        let (mut journal, _runtime) = supervisor.into_parts();
        assert!(matches!(
            journal.read(scope, sequence).unwrap().unwrap().event,
            TaskExecutionEvent::FaultObserved {
                fault: TaskFault::ProtectionViolation
            }
        ));
    }
}
#[test]
fn kernel_rejected_bootstrap_requires_reconstruction_and_cannot_complete() {
    let directory = tempfile::tempdir().unwrap();
    // Structurally recognizable static ELF, but no loadable segment. The image
    // admission parser accepts its identity; the kernel rejects execution.
    let mut bytes = [0_u8; 120];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&2_u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62_u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
    bytes[32..40].copy_from_slice(&64_u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64_u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56_u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1_u16.to_le_bytes());
    let image = directory.path().join("rejected-worker");
    std::fs::write(&image, bytes).unwrap();
    let mut definition = definition(2);
    definition.code = Digest::of(&bytes);
    let input = [0, 1, 2, 3];
    let scope = scope(definition, &input);
    let database = directory.path().join("tasks.db");
    let journal = SqliteTaskJournal::open(&database, definition, SqliteLimits::default()).unwrap();
    let authority =
        SqliteTaskJournal::open(&database, definition, SqliteLimits::default()).unwrap();
    let mut configuration = config(directory.path());
    configuration.worker = image.clone();
    let runtime = Runtime::new(authority, configuration).unwrap();
    let mut supervisor =
        Supervisor::restore(journal, runtime, scope, definition, TaskTick::ZERO).unwrap();
    supervisor.submit(&input, 1, TaskTick::ZERO).unwrap();
    assert_eq!(
        supervisor.advance(TaskTick::new(1)),
        Err(SupervisorError::Port(TaskPortError::Runtime))
    );
    assert_eq!(
        supervisor.advance(TaskTick::new(2)),
        Err(SupervisorError::ReconstructionRequired)
    );
    let (retained_journal, failed_runtime) = supervisor.into_parts();
    drop(failed_runtime);
    let restored_authority =
        SqliteTaskJournal::open(&database, definition, SqliteLimits::default()).unwrap();
    let mut restored_configuration = config(directory.path());
    restored_configuration.worker = image;
    let restored_runtime = Runtime::new(restored_authority, restored_configuration).unwrap();
    let restored_supervisor = Supervisor::restore(
        retained_journal,
        restored_runtime,
        scope,
        definition,
        TaskTick::new(2),
    )
    .unwrap();
    assert_eq!(
        restored_supervisor.projection().status(),
        TaskStatus::Faulted
    );
    assert_eq!(
        restored_supervisor.projection().generation(),
        TaskGeneration::new(1)
    );
    let (mut restored_journal, _restored_runtime) = restored_supervisor.into_parts();
    assert!(matches!(
        restored_journal
            .read(scope, TaskSequence::new(2))
            .unwrap()
            .unwrap()
            .event,
        TaskExecutionEvent::FaultObserved {
            fault: TaskFault::SupervisorInterrupted
        }
    ));
    assert_eq!(
        restored_journal.read(scope, TaskSequence::new(3)).unwrap(),
        None
    );
}
#[test]
fn kernel_address_space_limit_allows_small_and_rejects_large_allocations() {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[10, 1, 2, 3], 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    // Both mmap attempts occur in the worker. No poll callback can enforce its
    // memory admission, and merely reserving the oversized mapping must fail.
    wait(&mut supervisor).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
    let output = supervisor.projection().output().unwrap();
    let scope = supervisor.projection().scope();
    let (mut journal, _runtime) = supervisor.into_parts();
    let mut bytes = [0; 8];
    journal.read_blob(scope, output, &mut bytes).unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 6);
}
#[test]
fn checkpoint_requires_exact_commit_ack_before_completion() {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[5, 4, 5], 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    wait(&mut supervisor).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Running);
    assert!(supervisor.projection().checkpoint().is_some());
    wait(&mut supervisor).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
}
#[test]
fn corrupt_scope_and_oversized_frames_cannot_complete() {
    for mode in [6, 12] {
        let directory = tempfile::tempdir().unwrap();
        let mut supervisor = fresh(directory.path(), &[mode], 2);
        supervisor.advance(TaskTick::new(1)).unwrap();
        assert!(wait(&mut supervisor).is_err());
        assert_ne!(supervisor.projection().status(), TaskStatus::Completed);
        let projection = *supervisor.projection();
        let (mut journal, mut runtime) = supervisor.into_parts();
        runtime
            .fence(projection.scope(), projection.generation())
            .unwrap();
        assert_eq!(
            journal
                .read(projection.scope(), projection.next_sequence())
                .unwrap(),
            None
        );
    }
}
#[test]
fn restored_runtime_fences_live_generation_and_rejects_delayed_start() {
    let directory = tempfile::tempdir().unwrap();
    let input = [9];
    let mut supervisor = fresh(directory.path(), &input, 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    let projection = *supervisor.projection();
    let (journal, runtime) = supervisor.into_parts();
    drop(runtime);
    let authority = SqliteTaskJournal::open(
        &directory.path().join("tasks.db"),
        projection.definition(),
        SqliteLimits::default(),
    )
    .unwrap();
    let restored_runtime = Runtime::new(authority, config(directory.path())).unwrap();
    let recovered = Supervisor::restore(
        journal,
        restored_runtime,
        projection.scope(),
        projection.definition(),
        TaskTick::new(3),
    )
    .unwrap();
    assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
    let (_journal, mut restored) = recovered.into_parts();
    assert_eq!(
        restored.start(TaskStart {
            scope: projection.scope(),
            generation: projection.generation(),
            definition: projection.definition(),
            execution_deadline: projection.execution_deadline(),
            input_schema: 1,
            input: &input,
            checkpoint: None
        }),
        Err(TaskPortError::Conflict)
    );
}
#[test]
fn cpu_budget_is_enforced_by_kernel_without_supervisor_polling() {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[1], 1);
    supervisor.advance(TaskTick::new(1)).unwrap();
    std::thread::sleep(Duration::from_millis(1600));
    wait(&mut supervisor).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Faulted);
}
#[test]
fn exit_zero_without_result_is_not_completion() {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[7], 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    wait(&mut supervisor).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Faulted);
}

// Invoked as an isolated supervisor so the parent can SIGKILL it without
// terminating the test runner or relying on orderly destructors.
#[test]
fn supervisor_crash_helper() {
    let Some(directory) = std::env::var_os("PENELOPE_RUNTIME_CRASH_DIRECTORY") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let mut supervisor = fresh(&directory, &[9], 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    std::fs::write(directory.join("started"), b"ready").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn killed_supervisor_reconstructs_and_fences_without_clean_shutdown() {
    let directory = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "supervisor_crash_helper", "--nocapture"])
        .env("PENELOPE_RUNTIME_CRASH_DIRECTORY", directory.path())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !directory.path().join("started").exists() {
        if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
            let _kill_result = child.kill();
            let _wait_result = child.wait();
            panic!("isolated supervisor did not start its worker");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let definition = definition(2);
    let path = directory.path().join("tasks.db");
    let journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let runtime = Runtime::new(authority, config(directory.path())).unwrap();
    let recovered = Supervisor::restore(
        journal,
        runtime,
        scope(definition, &[9]),
        definition,
        TaskTick::new(3),
    )
    .unwrap();
    assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
    // A second reconstruction must retain the acknowledged termination.
    let (recovered_journal, recovered_runtime) = recovered.into_parts();
    let twice_recovered = Supervisor::restore(
        recovered_journal,
        recovered_runtime,
        scope(definition, &[9]),
        definition,
        TaskTick::new(4),
    )
    .unwrap();
    assert_eq!(twice_recovered.projection().status(), TaskStatus::Faulted);
}

#[test]
fn corrupt_registry_blocks_fence_until_evidence_is_restored() {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[9], 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    let projection = *supervisor.projection();
    let (_journal, mut runtime) = supervisor.into_parts();
    let manifest = std::fs::read_dir(directory.path().join("registry"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap() != "registry.lock")
        .unwrap();
    let saved = std::fs::read(&manifest).unwrap();
    std::fs::write(&manifest, b"truncated").unwrap();
    assert_eq!(
        runtime.fence(projection.scope(), projection.generation()),
        Err(TaskPortError::Corrupt)
    );
    // A tombstoned launch cannot resume even while termination is unconfirmed.
    assert_eq!(
        runtime.start(TaskStart {
            scope: projection.scope(),
            generation: projection.generation(),
            definition: projection.definition(),
            execution_deadline: projection.execution_deadline(),
            input_schema: 1,
            input: &[9],
            checkpoint: None,
        }),
        Err(TaskPortError::Conflict)
    );
    std::fs::write(manifest, saved).unwrap();
    runtime
        .fence(projection.scope(), projection.generation())
        .unwrap();
    runtime
        .fence(projection.scope(), projection.generation())
        .unwrap();
}

#[test]
fn unacknowledged_checkpoint_cannot_advance_worker() {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[5, 4, 5], 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    let projection = *supervisor.projection();
    let (_journal, mut runtime) = supervisor.into_parts();
    let mut buffer = [0; 64];
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let observation = runtime
            .poll(projection.scope(), projection.generation(), &mut buffer)
            .unwrap();
        if matches!(observation.progress, RuntimeProgress::Checkpoint { .. }) {
            break;
        }
        assert_eq!(observation.progress, RuntimeProgress::Pending);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        runtime.acknowledge_checkpoint(
            projection.scope(),
            projection.generation(),
            BlobRef {
                digest: Digest([0; 32]),
                bytes: 8,
                schema: 1
            }
        ),
        Err(TaskPortError::Conflict)
    );
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(
            runtime
                .poll(projection.scope(), projection.generation(), &mut buffer)
                .unwrap()
                .progress,
            RuntimeProgress::Pending
        );
    }
    runtime
        .fence(projection.scope(), projection.generation())
        .unwrap();
}

#[test]
fn committed_dispatch_recovers_task_intent_in_shared_and_separate_databases() {
    use penelope_domain::*;
    use penelope_executor::composition::commit_task_effect;
    use penelope_ports::{tasks::TaskEffectBinding, *};
    use penelope_sqlite::{SqliteLeaseClock, SqliteProcessStore};
    use std::{
        future::Future,
        num::NonZeroU16,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };
    struct SumClassifier;
    impl penelope_ports::tasks::TaskOutputClassifier for SumClassifier {
        fn identity(&self) -> Digest {
            Digest([4; 32])
        }
        fn classify(
            &self,
            _binding: &TaskEffectBinding,
            output: BlobRef,
            bytes: &[u8],
        ) -> Result<ExternalEffectState, penelope_ports::tasks::TaskEffectVerificationError>
        {
            if output.schema != 1 || bytes != 6_u64.to_le_bytes() {
                return Err(penelope_ports::tasks::TaskEffectVerificationError::InvalidOutput);
            }
            Ok(ExternalEffectState::Succeeded)
        }
    }
    struct Clock;
    impl SqliteLeaseClock for Clock {
        fn now(&self) -> Result<LogicalTime, PortError> {
            Ok(LogicalTime(1))
        }
    }
    struct Notify(std::thread::Thread);
    impl Wake for Notify {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    fn run<T>(future: impl Future<Output = T>) -> T {
        let mut future = Box::pin(future);
        let waker = Waker::from(Arc::new(Notify(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
            assert!(Instant::now() < deadline);
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }
    for separate in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let task_path = directory.path().join("tasks.db");
        let process_path = if separate {
            directory.path().join("process.db")
        } else {
            task_path.clone()
        };
        let store = SqliteProcessStore::open(
            &process_path,
            SqliteLimits::default(),
            Arc::new(Clock),
            100,
            8,
        )
        .unwrap();
        let process_scope = ProcessScope::new(
            "tnt_demo".try_into().unwrap(),
            "prc_demo".try_into().unwrap(),
            "def_demo".try_into().unwrap(),
            "dfv_one".try_into().unwrap(),
            ContentDigest([1; 32]),
        );
        let input = [0, 1, 2, 3];
        let action = ProcessAction::new(
            process_scope.clone(),
            "act_work".try_into().unwrap(),
            "stp_work".try_into().unwrap(),
            0,
            ProcessActionKind::ExternalEffect,
            ContentDigest::sha256(&input),
        );
        let outcome = ProcessOutcome::new(
            process_scope.clone(),
            0,
            ProcessOutcomeFact::new(
                "out_start".try_into().unwrap(),
                CausationId::Input("inp_start".try_into().unwrap()),
                OutcomeActor::System,
                LogicalTime(0),
                ProcessOutcomeKind::Started,
                ContentDigest([2; 32]),
            ),
        );
        let process_input = ProcessInput::new(
            process_scope.tenant_id.clone(),
            process_scope.process_id.clone(),
            "inp_start".try_into().unwrap(),
            ProcessInputKind::CanonicalEvent,
            ContentDigest([3; 32]),
        );
        let commit =
            AtomicProcessCommit::new(0, Some(process_input), vec![outcome], vec![action.clone()])
                .unwrap();
        // Simulate a process commit whose acknowledgement was lost.
        run(store.commit(&commit)).unwrap();
        let task_definition = definition(2);
        let binding =
            TaskEffectBinding::new(EffectDispatchRequest::new(action), task_definition).unwrap();
        let committed = run(commit_task_effect(&store, &commit, binding.clone())).unwrap();
        assert_eq!(committed.binding(), &binding);
        let empty_journal =
            SqliteTaskJournal::open(&task_path, task_definition, SqliteLimits::default()).unwrap();
        let empty_authority =
            SqliteTaskJournal::open(&task_path, task_definition, SqliteLimits::default()).unwrap();
        let empty_runtime = Runtime::new(empty_authority, config(directory.path())).unwrap();
        let missing_intent: Result<Supervisor, _> =
            committed.recover(empty_journal, empty_runtime, &input, 1, TaskTick::ZERO);
        assert!(matches!(
            missing_intent,
            Err(penelope_executor::composition::TaskCompositionError::BindingMismatch)
        ));

        let claim = OutboxClaimRequest::new(
            process_scope,
            "pri_worker".try_into().unwrap(),
            NonZeroU16::new(1).unwrap(),
        )
        .unwrap();
        let lease = run(store.claim(&claim)).unwrap().pop().unwrap();
        // Simulate durable task acceptance before the saga outbox ACK was sent.
        let journal =
            SqliteTaskJournal::open(&task_path, task_definition, SqliteLimits::default()).unwrap();
        let authority =
            SqliteTaskJournal::open(&task_path, task_definition, SqliteLimits::default()).unwrap();
        let runtime = Runtime::new(authority, config(directory.path())).unwrap();
        let mut accepted = Supervisor::restore(
            journal,
            runtime,
            binding.scope(),
            task_definition,
            TaskTick::ZERO,
        )
        .unwrap();
        accepted.submit(&input, 1, TaskTick::ZERO).unwrap();
        let (accepted_journal, accepted_runtime) = accepted.into_parts();
        let rejected: Result<Supervisor, _> = run(committed.accept(
            &store,
            &claim,
            &lease,
            LogicalTime(1),
            accepted_journal,
            accepted_runtime,
            &input,
            2,
            TaskTick::ZERO,
        ));
        assert!(rejected.is_err());
        let reopened_journal =
            SqliteTaskJournal::open(&task_path, task_definition, SqliteLimits::default()).unwrap();
        let reopened_authority =
            SqliteTaskJournal::open(&task_path, task_definition, SqliteLimits::default()).unwrap();
        let reopened_runtime = Runtime::new(reopened_authority, config(directory.path())).unwrap();
        let mut recovered: Supervisor = run(committed.accept(
            &store,
            &claim,
            &lease,
            LogicalTime(1),
            reopened_journal,
            reopened_runtime,
            &input,
            1,
            TaskTick::ZERO,
        ))
        .unwrap();
        assert_eq!(recovered.projection().next_sequence(), TaskSequence::new(1));
        assert!(run(store.claim(&claim)).unwrap().is_empty());
        recovered.advance(TaskTick::new(1)).unwrap();
        wait(&mut recovered).unwrap();
        assert_eq!(recovered.projection().status(), TaskStatus::Completed);
        let completed = *recovered.projection();
        let (mut completed_journal, _completed_runtime) = recovered.into_parts();
        let mut bytes = [0; 8];
        completed_journal
            .read_blob(binding.scope(), completed.output().unwrap(), &mut bytes)
            .unwrap();
        let verified = penelope_ports::tasks::VerifiedTaskEffectOutcome::verify(
            &binding,
            &completed,
            completed.generation(),
            &bytes,
            &SumClassifier,
            None,
        )
        .unwrap();
        let relay = penelope_executor::composition::TaskResultRelay::new(verified.clone()).unwrap();
        let reconstructed_relay =
            penelope_executor::composition::TaskResultRelay::new(verified).unwrap();
        assert_eq!(relay.input(), reconstructed_relay.input());
        assert_eq!(
            relay.observation().result,
            penelope_executor::engine::ActionResult::Succeeded
        );
        let result_outcome = ProcessOutcome::new(
            claim.scope.clone(),
            1,
            ProcessOutcomeFact::new(
                "out_result".try_into().unwrap(),
                CausationId::Input(relay.input().input_id.clone()),
                OutcomeActor::System,
                LogicalTime(2),
                ProcessOutcomeKind::ActionSucceeded,
                relay.input().payload_digest,
            ),
        );
        let result_commit =
            AtomicProcessCommit::new(1, Some(relay.input().clone()), vec![result_outcome], vec![])
                .unwrap();
        // Lose the result receipt; independently reconstruct the relay and retry.
        run(relay.commit(&store, &result_commit)).unwrap();
        assert!(
            run(reconstructed_relay.commit(&store, &result_commit))
                .unwrap()
                .duplicate_input
        );
        let mut substituted = result_commit.clone();
        substituted.input.as_mut().unwrap().input_id = "inp_substituted".try_into().unwrap();
        assert!(run(relay.commit(&store, &substituted)).is_err());
        let replay =
            OutcomeReplayRequest::new(claim.scope.clone(), 0, NonZeroU16::new(10).unwrap())
                .unwrap();
        assert_eq!(run(store.read_outcomes(&replay)).unwrap().outcomes.len(), 2);
    }
}

#[path = "support/sagas.rs"]
mod sagas;

#[path = "support/worker_protocol.rs"]
mod worker_protocol;

#[path = "support/launch_contracts.rs"]
mod launch_contracts;

#[path = "support/checkpoint_state.rs"]
mod checkpoint_state;

#[path = "support/fence_contracts.rs"]
mod fence_contracts;

#[path = "support/concurrency.rs"]
mod concurrency;
