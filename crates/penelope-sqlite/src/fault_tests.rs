//! Actual process death inside production journal transactions. SQLite's test
//! scalar function pauses at triggers; no production failpoint is compiled in.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use super::*;
use rusqlite::functions::FunctionFlags;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const fn definition() -> TaskDefinition {
    TaskDefinition {
        code: Digest([3; 32]),
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
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(10000),
        },
    }
}
fn scope() -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition().fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(b"input")),
    }
}
fn checkpoint() -> TaskRecord {
    TaskRecord {
        version: 1,
        scope: scope(),
        sequence: TaskSequence::new(2),
        generation: TaskGeneration::new(1),
        tick: TaskTick::new(2),
        event: TaskExecutionEvent::CheckpointCommitted {
            checkpoint: BlobRef {
                digest: Digest::of(b"checkpoint"),
                bytes: 10,
                schema: 1,
            },
        },
    }
}
fn journal(path: &Path) -> SqliteTaskJournal {
    SqliteTaskJournal::open(path, definition(), SqliteLimits::default()).unwrap()
}
fn pause(marker: &Path) -> ! {
    std::fs::write(marker, b"paused").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn crash_worker() {
    let Ok(database) = std::env::var("PENELOPE_TX_CRASH_DB") else {
        return;
    };
    let marker = PathBuf::from(std::env::var("PENELOPE_TX_CRASH_MARKER").unwrap());
    let boundary = std::env::var("PENELOPE_TX_CRASH_BOUNDARY").unwrap();
    let mut adapter = journal(Path::new(&database));
    if boundary != "postcommit" {
        let trigger = match boundary.as_str() {
            "preblob" => {
                "CREATE TEMP TRIGGER pause_write BEFORE INSERT ON task_blobs BEGIN SELECT pause_test(); END;"
            }
            "postblob" => {
                "CREATE TEMP TRIGGER pause_write AFTER INSERT ON task_blobs BEGIN SELECT pause_test(); END;"
            }
            "precommit" => {
                "CREATE TEMP TRIGGER pause_write AFTER INSERT ON task_records BEGIN SELECT pause_test(); END;"
            }
            _ => panic!("unknown crash boundary"),
        };
        let marker = marker.clone();
        adapter
            .connection
            .create_scalar_function(
                "pause_test",
                0,
                FunctionFlags::SQLITE_UTF8,
                move |_context| -> rusqlite::Result<i32> {
                    pause(&marker);
                },
            )
            .unwrap();
        adapter.connection.execute_batch(trigger).unwrap();
    }
    adapter.commit(&checkpoint(), Some(b"checkpoint")).unwrap();
    pause(&marker);
}

#[test]
fn process_kill_at_each_transaction_boundary_preserves_atomicity() {
    for boundary in ["preblob", "postblob", "precommit", "postcommit"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tasks.db");
        let marker = directory.path().join("marker");
        let mut adapter = journal(&path);
        let mut projection = TaskProjection::new(scope(), definition()).unwrap();
        let intent = projection
            .propose(
                TaskExecutionEvent::IntentCommitted {
                    definition: definition(),
                    input: BlobRef {
                        digest: Digest::of(b"input"),
                        bytes: 5,
                        schema: 1,
                    },
                },
                TaskGeneration::ZERO,
                TaskTick::ZERO,
            )
            .unwrap();
        adapter.commit(&intent, Some(b"input")).unwrap();
        projection.apply(&intent).unwrap();
        adapter
            .commit(&projection.next(TaskTick::new(1)).unwrap().unwrap(), None)
            .unwrap();
        drop(adapter);
        let mut worker = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tasks::fault_tests::crash_worker", "--nocapture"])
            .env("PENELOPE_TX_CRASH_DB", &path)
            .env("PENELOPE_TX_CRASH_MARKER", &marker)
            .env("PENELOPE_TX_CRASH_BOUNDARY", boundary)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.exists() {
            if Instant::now() >= deadline {
                let _kill = worker.kill();
                let _wait = worker.wait();
                panic!("worker failed to reach {boundary}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        worker.kill().unwrap();
        worker.wait().unwrap();
        let mut reopened = journal(&path);
        reopened.check_integrity().unwrap();
        let record = reopened.read(scope(), TaskSequence::new(2)).unwrap();
        let count: u32 = reopened
            .connection
            .query_row("SELECT count(*) FROM task_blobs", [], |row| row.get(0))
            .unwrap();
        if boundary == "postcommit" {
            assert_eq!(record, Some(checkpoint()));
            assert_eq!(count, 2);
        } else {
            assert_eq!(record, None);
            assert_eq!(count, 1);
        }
        reopened.rebuild_head(scope()).unwrap();
    }
}
