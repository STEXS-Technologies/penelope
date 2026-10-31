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

// SQL views/functions inject engine read errors without changing production SQL
// or depending on filesystem failure timing. Restore each table before asserting.
fn read_error_case(table: &str, code: i32) -> TaskPortError {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("read-errors.db");
    let mut store = journal(&path);
    let reference = BlobRef {
        digest: Digest::of(b"input"),
        bytes: 5,
        schema: 1,
    };
    let record = TaskProjection::new(scope(), definition())
        .unwrap()
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition: definition(),
                input: reference,
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        )
        .unwrap();
    store.commit(&record, Some(b"input")).unwrap();
    store
        .connection
        .create_scalar_function(
            "fail_read",
            1,
            FunctionFlags::SQLITE_UTF8,
            move |_context| -> rusqlite::Result<Vec<u8>> {
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(code),
                    None,
                ))
            },
        )
        .unwrap();
    let columns = match table {
        "task_heads" => "scope,next_sequence,fail_read(last_digest) AS last_digest,bytes_used",
        "task_records" => "scope,sequence,fail_read(record) AS record,checksum",
        "task_blobs" => "scope,digest,fail_read(bytes) AS bytes",
        _ => panic!("unknown read fault table"),
    };
    store.connection.execute_batch(&format!(
        "ALTER TABLE {table} RENAME TO saved_read_table; CREATE VIEW {table} AS SELECT {columns} FROM saved_read_table;"
    )).unwrap();
    let rejected = if table == "task_blobs" {
        store
            .read_blob(scope(), reference, &mut [0; 64])
            .unwrap_err()
    } else {
        store.read(scope(), TaskSequence::ZERO).unwrap_err()
    };
    store
        .connection
        .execute_batch(&format!(
            "DROP VIEW {table}; ALTER TABLE saved_read_table RENAME TO {table};"
        ))
        .unwrap();
    // The same connection and a fresh connection must both work after the
    // injected storage failure is removed; the committed state stays intact.
    assert_eq!(
        store.read(scope(), TaskSequence::ZERO).unwrap(),
        Some(record)
    );
    let mut bytes = [0; 64];
    assert_eq!(store.read_blob(scope(), reference, &mut bytes).unwrap(), 5);
    assert_eq!(bytes.get(..5).unwrap(), b"input");
    drop(store);
    assert_eq!(
        journal(&path).read(scope(), TaskSequence::ZERO).unwrap(),
        Some(record)
    );
    rejected
}
fn read_error_matrix(table: &str) {
    let mut failures = Vec::new();
    for (code, expected) in [
        (rusqlite::ffi::SQLITE_IOERR, TaskPortError::Unavailable),
        (rusqlite::ffi::SQLITE_BUSY, TaskPortError::Unavailable),
        (rusqlite::ffi::SQLITE_LOCKED, TaskPortError::Unavailable),
        (rusqlite::ffi::SQLITE_FULL, TaskPortError::Exhausted),
        (rusqlite::ffi::SQLITE_TOOBIG, TaskPortError::Exhausted),
        (rusqlite::ffi::SQLITE_CORRUPT, TaskPortError::Corrupt),
        (rusqlite::ffi::SQLITE_NOTADB, TaskPortError::Corrupt),
    ] {
        let actual = read_error_case(table, code);
        println!(
            "{table} SQLite code {code}: {actual:?}, expected {expected:?}; repair/reopen passed"
        );
        if actual != expected {
            failures.push((code, actual, expected));
        }
    }
    assert!(failures.is_empty(), "{table}: {failures:?}");
}
#[test]
fn head_reads_preserve_engine_error_categories_and_recover() {
    read_error_matrix("task_heads");
}
#[test]
fn record_reads_preserve_engine_error_categories_and_recover() {
    read_error_matrix("task_records");
}
#[test]
fn blob_reads_preserve_engine_error_categories_and_recover() {
    read_error_matrix("task_blobs");
}
