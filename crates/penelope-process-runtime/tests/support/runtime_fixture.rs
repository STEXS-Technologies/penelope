//! Shared real Linux worker fixture for runtime and optional upstream conformance.
use penelope_core::task::*;
use penelope_executor::task::TaskSupervisor;
use penelope_process_runtime::{LinuxProcessRuntime, ProcessRuntimeConfig};
use penelope_sqlite::SqliteTaskJournal;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::Duration,
};

pub(super) fn fixture() -> PathBuf {
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
pub(super) fn definition(cpu: u64) -> TaskDefinition {
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
pub(super) fn config(directory: &Path) -> ProcessRuntimeConfig {
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
pub(super) type Runtime = LinuxProcessRuntime<SqliteTaskJournal>;
pub(super) type Supervisor = TaskSupervisor<SqliteTaskJournal, Runtime, 64>;
