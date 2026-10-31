#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
use rusqlite::Connection;

const fn definition() -> TaskDefinition {
    TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromBeginning,
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
fn intent() -> TaskRecord {
    TaskProjection::new(scope(), definition())
        .unwrap()
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
        .unwrap()
}
fn open(path: &std::path::Path) -> SqliteTaskJournal {
    SqliteTaskJournal::open(path, definition(), SqliteLimits::default()).unwrap()
}

#[test]
fn atomic_blob_history_commit_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    let record = intent();
    assert_eq!(
        journal.commit(&record, Some(b"wrong")),
        Err(TaskPortError::Corrupt)
    );
    assert_eq!(journal.read(scope(), TaskSequence::ZERO).unwrap(), None);
    journal.commit(&record, Some(b"input")).unwrap();
    drop(journal);
    let mut reopened = open(&path);
    assert_eq!(
        reopened.read(scope(), TaskSequence::ZERO).unwrap(),
        Some(record)
    );
    assert_eq!(reopened.read(scope(), TaskSequence::new(1)).unwrap(), None);
    let mut bytes = [0; 5];
    let reference = BlobRef {
        digest: Digest::of(b"input"),
        bytes: 5,
        schema: 1,
    };
    assert_eq!(
        reopened.read_blob(scope(), reference, &mut bytes).unwrap(),
        5
    );
    assert_eq!(&bytes, b"input");
    assert_eq!(
        reopened.commit(&record, Some(b"input")),
        Err(TaskPortError::Conflict)
    );
}

// Spawn both barrier participants before joining either thread.
#[allow(clippy::needless_collect)]
#[test]
fn concurrent_connections_choose_one_cas_winner() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    drop(open(&path));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut journal = open(&path);
                barrier.wait();
                journal.commit(&intent(), Some(b"input"))
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(TaskPortError::Conflict))
            .count(),
        1
    );
}

#[test]
fn gaps_corrupt_bytes_missing_head_and_future_schema_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    journal.commit(&intent(), Some(b"input")).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute("UPDATE task_records SET checksum=zeroblob(32)", [])
        .unwrap();
    assert_eq!(
        journal.read(scope(), TaskSequence::ZERO),
        Err(TaskPortError::Corrupt)
    );
    connection.execute("DELETE FROM task_records", []).unwrap();
    assert_eq!(
        journal.read(scope(), TaskSequence::ZERO),
        Err(TaskPortError::Corrupt)
    );
    connection.pragma_update(None, "user_version", 999).unwrap();
    assert!(matches!(
        SqliteTaskJournal::open(&path, definition(), SqliteLimits::default()),
        Err(TaskPortError::Corrupt)
    ));
}

#[test]
fn explicit_head_rebuild_validates_log_and_blobs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    journal.commit(&intent(), Some(b"input")).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection.execute("DELETE FROM task_heads", []).unwrap();
    assert_eq!(
        journal.read(scope(), TaskSequence::ZERO),
        Err(TaskPortError::Corrupt)
    );
    journal.rebuild_head(scope()).unwrap();
    assert_eq!(
        journal.read(scope(), TaskSequence::ZERO).unwrap(),
        Some(intent())
    );
    connection
        .execute("UPDATE task_blobs SET bytes=zeroblob(100000)", [])
        .unwrap();
    assert_eq!(journal.rebuild_head(scope()), Err(TaskPortError::Corrupt));
}

#[test]
fn fencing_survives_reopen_and_blocks_delayed_start() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    let record = intent();
    journal.commit(&record, Some(b"input")).unwrap();
    let mut projection = TaskProjection::new(scope(), definition()).unwrap();
    projection.apply(&record).unwrap();
    let start = projection.next(TaskTick::new(1)).unwrap().unwrap();
    journal.commit(&start, None).unwrap();
    projection.apply(&start).unwrap();
    let request = TaskStart {
        scope: scope(),
        generation: projection.generation(),
        definition: definition(),
        execution_deadline: projection.execution_deadline(),
        input_schema: 1,
        input: b"input",
        checkpoint: None,
    };
    let execution = Digest([9; 32]);
    assert_eq!(
        journal.reserve_start(request, execution).unwrap(),
        StartReservation::Reserved
    );
    assert_eq!(
        journal.reserve_start(request, execution).unwrap(),
        StartReservation::AlreadyReserved
    );
    assert_eq!(
        journal.reserve_start(request, Digest([8; 32])),
        Err(TaskPortError::Conflict)
    );
    assert_eq!(
        journal
            .begin_fence(scope(), projection.generation())
            .unwrap(),
        Some(execution)
    );
    drop(journal);
    let mut reopened = open(&path);
    assert_eq!(
        reopened.reserve_start(request, execution),
        Err(TaskPortError::Conflict)
    );
    assert_eq!(
        reopened.confirm_stopped(scope(), projection.generation(), None),
        Err(TaskPortError::Conflict)
    );
    reopened
        .confirm_stopped(scope(), projection.generation(), Some(execution))
        .unwrap();
    assert_eq!(
        reopened.reserve_start(request, execution),
        Err(TaskPortError::Conflict)
    );
}

#[test]
fn scope_quota_rolls_back_blob_and_record_together() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let limits = SqliteLimits {
        max_scope_bytes: 1,
        ..SqliteLimits::default()
    };
    let mut journal = SqliteTaskJournal::open(&path, definition(), limits).unwrap();
    assert_eq!(
        journal.commit(&intent(), Some(b"input")),
        Err(TaskPortError::Exhausted)
    );
    assert_eq!(journal.read(scope(), TaskSequence::ZERO).unwrap(), None);
    let connection = Connection::open(&path).unwrap();
    let count: u32 = connection
        .query_row("SELECT count(*) FROM task_blobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

fn committed_start(journal: &mut SqliteTaskJournal) -> TaskProjection {
    let record = intent();
    journal.commit(&record, Some(b"input")).unwrap();
    let mut projection = TaskProjection::new(scope(), definition()).unwrap();
    projection.apply(&record).unwrap();
    let start = projection.next(TaskTick::new(1)).unwrap().unwrap();
    journal.commit(&start, None).unwrap();
    projection.apply(&start).unwrap();
    projection
}
fn request(projection: &TaskProjection) -> TaskStart<'static> {
    TaskStart {
        scope: scope(),
        generation: projection.generation(),
        definition: definition(),
        execution_deadline: projection.execution_deadline(),
        input_schema: 1,
        input: b"input",
        checkpoint: None,
    }
}

#[test]
fn successor_is_blocked_until_predecessor_termination_is_confirmed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    let mut projection = committed_start(&mut journal);
    let execution = Digest([9; 32]);
    journal
        .reserve_start(request(&projection), execution)
        .unwrap();
    journal
        .begin_fence(scope(), projection.generation())
        .unwrap();
    let fault = projection
        .propose(
            TaskExecutionEvent::FaultObserved {
                fault: TaskFault::Crash,
            },
            projection.generation(),
            TaskTick::new(2),
        )
        .unwrap();
    journal.commit(&fault, None).unwrap();
    projection.apply(&fault).unwrap();
    for tick in 3..=4 {
        let record = projection.next(TaskTick::new(tick)).unwrap().unwrap();
        journal.commit(&record, None).unwrap();
        projection.apply(&record).unwrap();
    }
    assert_eq!(projection.generation(), TaskGeneration::new(2));
    assert_eq!(
        journal.reserve_start(request(&projection), Digest([8; 32])),
        Err(TaskPortError::Conflict)
    );
    journal
        .confirm_stopped(scope(), TaskGeneration::new(1), Some(execution))
        .unwrap();
    assert_eq!(
        journal
            .reserve_start(request(&projection), Digest([8; 32]))
            .unwrap(),
        StartReservation::Reserved
    );
}

#[test]
fn authorizations_reject_uncommitted_and_substituted_requests() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    let empty = TaskProjection::new(scope(), definition()).unwrap();
    assert!(
        journal
            .reserve_start(request(&empty), Digest([9; 32]))
            .is_err()
    );
    let projection = committed_start(&mut journal);
    let valid = request(&projection);
    for invalid in [
        TaskStart {
            input: b"wrong",
            ..valid
        },
        TaskStart {
            input_schema: 2,
            ..valid
        },
        TaskStart {
            execution_deadline: TaskTick::new(999),
            ..valid
        },
        TaskStart {
            generation: TaskGeneration::new(2),
            ..valid
        },
        TaskStart {
            checkpoint: Some(b"forged"),
            ..valid
        },
    ] {
        assert!(journal.reserve_start(invalid, Digest([9; 32])).is_err());
    }
    let mut foreign = valid.scope;
    foreign.tenant = TaskTenantId::from_digest(Digest([7; 32]));
    assert!(
        journal
            .reserve_start(
                TaskStart {
                    scope: foreign,
                    ..valid
                },
                Digest([9; 32])
            )
            .is_err()
    );
    assert_eq!(
        journal.reserve_start(valid, Digest([9; 32])).unwrap(),
        StartReservation::Reserved
    );
}

#[test]
fn fence_before_dispatch_prevents_delayed_start_after_reconstruction() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    let projection = committed_start(&mut journal);
    assert_eq!(
        journal
            .begin_fence(scope(), projection.generation())
            .unwrap(),
        None
    );
    journal
        .confirm_stopped(scope(), projection.generation(), None)
        .unwrap();
    drop(journal);
    assert_eq!(
        open(&path).reserve_start(request(&projection), Digest([9; 32])),
        Err(TaskPortError::Conflict)
    );
}

#[test]
fn lock_contention_is_bounded_and_does_not_accept_input() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let limits = SqliteLimits {
        busy_timeout: std::time::Duration::from_millis(10),
        ..SqliteLimits::default()
    };
    let mut journal = SqliteTaskJournal::open(&path, definition(), limits).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        journal.commit(&intent(), Some(b"input")),
        Err(TaskPortError::Unavailable)
    );
    connection.execute_batch("ROLLBACK").unwrap();
    assert_eq!(journal.read(scope(), TaskSequence::ZERO).unwrap(), None);
    journal.commit(&intent(), Some(b"input")).unwrap();
}

#[test]
fn transaction_failure_after_blob_insert_rolls_back_everything() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_before_head BEFORE INSERT ON task_heads BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert_eq!(
        journal.commit(&intent(), Some(b"input")),
        Err(TaskPortError::Conflict)
    );
    let count: u32 = connection
        .query_row("SELECT count(*) FROM task_blobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(journal.read(scope(), TaskSequence::ZERO).unwrap(), None);
}

#[test]
fn sqlite_process_worker() {
    let Ok(path) = std::env::var("PENELOPE_SQLITE_TEST_DB") else {
        return;
    };
    let ready = std::env::var("PENELOPE_SQLITE_TEST_READY").unwrap();
    let gate = std::env::var("PENELOPE_SQLITE_TEST_GATE").unwrap();
    let mut journal = open(std::path::Path::new(&path));
    publish_marker(std::path::Path::new(&ready), b"ready");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !std::path::Path::new(&gate).exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    match journal.commit(&intent(), Some(b"input")) {
        Ok(_) => publish_marker(std::path::Path::new(&ready), b"committed"),
        Err(TaskPortError::Conflict) => {
            publish_marker(std::path::Path::new(&ready), b"conflict");
        }
        result => panic!("unexpected arbitration result: {result:?}"),
    }
    if std::env::var("PENELOPE_SQLITE_TEST_CRASH").is_ok() {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

fn publish_marker(path: &std::path::Path, state: &[u8]) {
    // Readers must see the old state or the complete new state, never the
    // transient empty/partial file produced by overwriting a live marker.
    let temporary = path.with_extension("next");
    std::fs::write(&temporary, state).unwrap();
    std::fs::rename(temporary, path).unwrap();
}

fn wait_marker(path: &std::path::Path, expected: &[u8]) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::fs::read(path).ok().as_deref() != Some(expected) {
        assert!(
            std::time::Instant::now() < deadline,
            "worker did not reach marker"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn distinct_processes_arbitrate_and_recover_after_unacknowledged_commit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    drop(open(&path));
    let gate = directory.path().join("gate");
    let first_marker = directory.path().join("first");
    let second_marker = directory.path().join("second");
    let spawn = |marker: &std::path::Path| {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sqlite_process_worker", "--nocapture"])
            .env("PENELOPE_SQLITE_TEST_DB", &path)
            .env("PENELOPE_SQLITE_TEST_GATE", &gate)
            .env("PENELOPE_SQLITE_TEST_READY", marker)
            .env("PENELOPE_SQLITE_TEST_CRASH", "1")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut first = spawn(&first_marker);
    let mut second = spawn(&second_marker);
    wait_marker(&first_marker, b"ready");
    wait_marker(&second_marker, b"ready");
    std::fs::write(&gate, b"go").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let first_state = std::fs::read(&first_marker).unwrap();
        let second_state = std::fs::read(&second_marker).unwrap();
        if first_state != b"ready" && second_state != b"ready" {
            assert!(
                first_state == b"committed" && second_state == b"conflict"
                    || first_state == b"conflict" && second_state == b"committed"
            );
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    first.kill().unwrap();
    second.kill().unwrap();
    first.wait().unwrap();
    second.wait().unwrap();
    let mut reopened = open(&path);
    assert_eq!(
        reopened.read(scope(), TaskSequence::ZERO).unwrap(),
        Some(intent())
    );
    assert_eq!(
        reopened.commit(&intent(), Some(b"input")),
        Err(TaskPortError::Conflict)
    );
}

#[test]
fn head_rebuild_cannot_hide_a_missing_committed_tail() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    committed_start(&mut journal);
    Connection::open(&path)
        .unwrap()
        .execute("DELETE FROM task_records WHERE sequence=1", [])
        .unwrap();
    assert_eq!(journal.rebuild_head(scope()), Err(TaskPortError::Corrupt));
    assert_eq!(
        journal.read(scope(), TaskSequence::new(1)),
        Err(TaskPortError::Corrupt)
    );
}

#[test]
fn retirement_preserves_fences_and_prevents_resurrection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    let mut projection = committed_start(&mut journal);
    assert_eq!(
        journal.retire_task(scope(), projection.next_sequence()),
        Err(TaskPortError::Conflict)
    );
    let execution = Digest([9; 32]);
    journal
        .reserve_start(request(&projection), execution)
        .unwrap();
    let output = BlobRef {
        digest: Digest::of(b"done"),
        bytes: 4,
        schema: 1,
    };
    let complete = projection
        .propose(
            TaskExecutionEvent::Completed { output },
            projection.generation(),
            TaskTick::new(2),
        )
        .unwrap();
    journal.commit(&complete, Some(b"done")).unwrap();
    projection.apply(&complete).unwrap();
    assert_eq!(
        journal.retire_task(scope(), projection.next_sequence()),
        Err(TaskPortError::Conflict)
    );
    journal
        .begin_fence(scope(), projection.generation())
        .unwrap();
    journal
        .confirm_stopped(scope(), projection.generation(), Some(execution))
        .unwrap();
    assert_eq!(
        journal.retire_task(scope(), TaskSequence::new(2)),
        Err(TaskPortError::Conflict)
    );
    let retired = journal
        .retire_task(scope(), projection.next_sequence())
        .unwrap();
    assert_eq!(retired.next_sequence, TaskSequence::new(3));
    drop(journal);
    let mut reopened = open(&path);
    assert_eq!(
        reopened.read(scope(), TaskSequence::ZERO),
        Err(TaskPortError::Conflict)
    );
    assert_eq!(
        reopened.commit(&intent(), Some(b"input")),
        Err(TaskPortError::Conflict)
    );
    assert_eq!(reopened.rebuild_head(scope()), Err(TaskPortError::Conflict));
    assert_eq!(
        reopened.read_blob(scope(), output, &mut [0; 4]),
        Err(TaskPortError::Conflict)
    );
    reopened.check_integrity().unwrap();
    assert_eq!(reopened.checkpoint_wal().unwrap().busy, 0);
    let connection = Connection::open(&path).unwrap();
    let count: u32 = connection
        .query_row("SELECT count(*) FROM task_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let fences: u32 = connection
        .query_row("SELECT count(*) FROM runtime_generations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(fences, 1);
}

#[test]
fn schema_upgrade_preserves_v1_task_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let mut journal = open(&path);
    journal.commit(&intent(), Some(b"input")).unwrap();
    drop(journal);
    let connection = Connection::open(&path).unwrap();
    let before: Vec<u8> = connection
        .query_row("SELECT record FROM task_records", [], |row| row.get(0))
        .unwrap();
    connection
        .execute_batch("DROP TABLE task_retirements; DROP TABLE process_payloads; DROP TABLE process_usage; DROP TABLE process_heads; DROP TABLE process_outcomes; DROP TABLE process_inbox; DROP TABLE process_sources; DROP TABLE process_commits; DROP TABLE process_actions; DROP TABLE process_timers; DROP TABLE lease_counter; PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    let mut upgraded = open(&path);
    assert_eq!(
        upgraded.read(scope(), TaskSequence::ZERO).unwrap(),
        Some(intent())
    );
    drop(upgraded);
    let migrated_connection = Connection::open(&path).unwrap();
    let after: Vec<u8> = migrated_connection
        .query_row("SELECT record FROM task_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(before, after);
    let version: u32 = migrated_connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 6);
}

#[test]
fn failed_schema_upgrade_rolls_back_schema_and_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    drop(open(&path));
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("DROP TABLE task_retirements; PRAGMA user_version=1;")
        .unwrap();
    assert!(SqliteTaskJournal::open(&path, definition(), SqliteLimits::default()).is_err());
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let retired_table: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='task_retirements')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!retired_table);
}

#[test]
fn structural_integrity_check_reports_constraint_corruption_and_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("integrity.db");
    let journal = open(&path);
    journal.check_integrity().unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE lease_counter SET next_token=0")
        .unwrap();
    assert_eq!(journal.check_integrity(), Err(TaskPortError::Corrupt));
    // Test-fixture repair is explicit; check_integrity must never repair state.
    let counter: i64 = connection
        .query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))
        .unwrap();
    assert_eq!(counter, 0);
    connection
        .execute("UPDATE lease_counter SET next_token=1", [])
        .unwrap();
    journal.check_integrity().unwrap();
}

#[path = "support/suppressed_task_writes.rs"]
mod suppressed_task_writes;
#[path = "support/write_snapshot.rs"]
mod write_snapshot;

#[path = "support/cache_usage.rs"]
mod cache_usage;

#[path = "support/task_frontier.rs"]
mod task_frontier;

#[path = "support/task_columns.rs"]
mod task_columns;

#[path = "support/task_append_frontier.rs"]
mod task_append_frontier;

#[path = "support/task_blob_columns.rs"]
mod task_blob_columns;
