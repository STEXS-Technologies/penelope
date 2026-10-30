#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]
use penelope_core::task::{runtime::*, *};
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use penelope_process_runtime::{LinuxProcessRuntime, ProcessRuntimeConfig};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::{Duration, Instant},
};

fn fixture() -> PathBuf {
    static FIXTURE: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let directory = tempfile::tempdir().unwrap();
            let binary = directory.path().join("worker");
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/worker.c");
            let status = Command::new("cc")
                .args([
                    "-nostdlib",
                    "-static",
                    "-fno-builtin",
                    "-fno-stack-protector",
                    "-fno-pie",
                    "-no-pie",
                    "-o",
                ])
                .arg(&binary)
                .arg(source)
                .status()
                .unwrap();
            assert!(status.success());
            (directory, binary)
        })
        .1
        .clone()
}
fn definition(cpu: u64) -> TaskDefinition {
    TaskDefinition {
        code: Digest::of(&std::fs::read(fixture()).unwrap()),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromCheckpoint,
            max_attempts: 3,
            timeout_ticks: 50,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 64,
        },
        budget: ExecutionBudget {
            max_input_bytes: 64,
            max_output_bytes: 64,
            max_history_records: 32,
            max_checkpoint_commits: 8,
            cpu_ticks: cpu,
            deadline_tick: TaskTick::new(1000),
        },
    }
}
fn scope(definition: TaskDefinition, input: &[u8]) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(input)),
    }
}
fn config(directory: &Path) -> ProcessRuntimeConfig {
    ProcessRuntimeConfig {
        registry: directory.join("registry"),
        worker: fixture(),
        max_worker_bytes: 1024 * 1024,
        max_frame_bytes: 64,
        max_live: 4,
        ticks_per_second: 1,
        address_space_bytes: 64 * 1024 * 1024,
        startup_timeout: Duration::from_secs(1),
        fence_timeout: Duration::from_secs(1),
    }
}
type Runtime = LinuxProcessRuntime<SqliteTaskJournal>;
type Supervisor = TaskSupervisor<SqliteTaskJournal, Runtime, 64>;
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
fn kernel_blocks_fork_filesystem_and_network_syscalls() {
    for mode in [2, 3, 4] {
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
