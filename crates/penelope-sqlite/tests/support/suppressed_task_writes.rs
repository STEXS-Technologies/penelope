//! Suppressed writes cannot authorize execution or acknowledge missing history.
use super::write_snapshot::snapshot;
use super::*;
const TABLES: &[&str] = &[
    "task_heads",
    "task_records",
    "task_blobs",
    "runtime_generations",
    "task_retirements",
];
fn completed(journal: &mut SqliteTaskJournal) -> TaskProjection {
    let mut projection = committed_start(journal);
    let record = projection
        .propose(
            TaskExecutionEvent::Completed {
                output: BlobRef {
                    digest: Digest::of(b"done"),
                    bytes: 4,
                    schema: 1,
                },
            },
            projection.generation(),
            TaskTick::new(2),
        )
        .unwrap();
    journal.commit(&record, Some(b"done")).unwrap();
    projection.apply(&record).unwrap();
    projection
}
#[test]
fn ignored_task_commit_writes_roll_back_blobs_heads_and_records() {
    let mut failures = Vec::new();
    for (table, operation, append, expected) in [
        ("task_heads", "INSERT", false, TaskPortError::Corrupt),
        ("task_records", "INSERT", false, TaskPortError::Corrupt),
        ("task_records", "INSERT", true, TaskPortError::Corrupt),
        ("task_blobs", "INSERT", false, TaskPortError::Corrupt),
        ("task_heads", "UPDATE", true, TaskPortError::Conflict),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("suppressed-task-commit.db");
        let mut journal = open(&path);
        let record = if append {
            let projection = committed_start(&mut journal);
            projection
                .propose(
                    TaskExecutionEvent::Completed {
                        output: BlobRef {
                            digest: Digest::of(b"done"),
                            bytes: 4,
                            schema: 1,
                        },
                    },
                    projection.generation(),
                    TaskTick::new(2),
                )
                .unwrap()
        } else {
            intent()
        };
        let blob: &[u8] = if append { b"done" } else { b"input" };
        let connection = Connection::open(&path).unwrap();
        let before = snapshot(&connection, TABLES);
        connection.execute_batch(&format!("CREATE TRIGGER suppress BEFORE {operation} ON {table} BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
        let result = journal.commit(&record, Some(blob));
        let unchanged = snapshot(&connection, TABLES) == before;
        connection.execute_batch("DROP TRIGGER suppress").unwrap();
        if result != Err(expected) || !unchanged {
            failures.push(format!(
                "{table}/{operation}/append={append}: {:?}, unchanged={unchanged}",
                result.map(|_record| ())
            ));
            continue;
        }
        assert_eq!(journal.commit(&record, Some(blob)).unwrap(), record);
        assert_eq!(
            journal.commit(&record, Some(blob)),
            Err(TaskPortError::Conflict)
        );
        assert_eq!(
            open(&path).read(scope(), record.sequence).unwrap(),
            Some(record)
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
#[test]
fn ignored_task_head_repair_cannot_populate_a_false_cache() {
    let mut failures = Vec::new();
    for missing in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("suppressed-rebuild.db");
        let mut journal = open(&path);
        let projection = committed_start(&mut journal);
        let connection = Connection::open(&path).unwrap();
        if missing {
            connection.execute("DELETE FROM task_heads", []).unwrap();
        } else {
            connection
                .execute("UPDATE task_heads SET last_digest=zeroblob(32)", [])
                .unwrap();
        }
        let before = snapshot(&connection, TABLES);
        let operation = if missing { "INSERT" } else { "UPDATE" };
        connection.execute_batch(&format!("CREATE TRIGGER suppress BEFORE {operation} ON task_heads BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
        let result = journal.rebuild_head(scope());
        let unchanged = snapshot(&connection, TABLES) == before;
        connection.execute_batch("DROP TRIGGER suppress").unwrap();
        if result != Err(TaskPortError::Corrupt) || !unchanged {
            failures.push(format!(
                "missing={missing}: {result:?}, unchanged={unchanged}"
            ));
            continue;
        }
        journal.rebuild_head(scope()).unwrap();
        journal.rebuild_head(scope()).unwrap();
        assert!(
            open(&path)
                .read(
                    scope(),
                    TaskSequence::new(projection.next_sequence().get().checked_sub(1).unwrap())
                )
                .unwrap()
                .is_some()
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
#[test]
fn ignored_task_authority_writes_cannot_grant_or_fence_a_generation() {
    let mut failures = Vec::new();
    for mode in ["reserve", "fence-missing", "fence-reserved"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("suppressed-authority.db");
        let mut journal = open(&path);
        let projection = committed_start(&mut journal);
        let execution = Digest([9; 32]);
        if mode == "fence-reserved" {
            journal
                .reserve_start(request(&projection), execution)
                .unwrap();
        }
        let connection = Connection::open(&path).unwrap();
        let before = snapshot(&connection, TABLES);
        let operation = if mode == "fence-reserved" {
            "UPDATE"
        } else {
            "INSERT"
        };
        connection.execute_batch(&format!("CREATE TRIGGER suppress BEFORE {operation} ON runtime_generations BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
        let result = if mode == "reserve" {
            journal
                .reserve_start(request(&projection), execution)
                .map(|_reservation| ())
        } else {
            journal
                .begin_fence(scope(), projection.generation())
                .map(|_execution| ())
        };
        let unchanged = snapshot(&connection, TABLES) == before;
        connection.execute_batch("DROP TRIGGER suppress").unwrap();
        if result != Err(TaskPortError::Corrupt) || !unchanged {
            failures.push(format!("{mode}: {result:?}, unchanged={unchanged}"));
            continue;
        }
        if mode == "reserve" {
            assert_eq!(
                journal
                    .reserve_start(request(&projection), execution)
                    .unwrap(),
                StartReservation::Reserved
            );
            assert_eq!(
                open(&path)
                    .reserve_start(request(&projection), execution)
                    .unwrap(),
                StartReservation::AlreadyReserved
            );
        } else {
            let id = journal
                .begin_fence(scope(), projection.generation())
                .unwrap();
            assert_eq!(
                id,
                if mode == "fence-reserved" {
                    Some(execution)
                } else {
                    None
                }
            );
            assert_eq!(
                open(&path).reserve_start(request(&projection), execution),
                Err(TaskPortError::Conflict)
            );
            journal
                .confirm_stopped(scope(), projection.generation(), id)
                .unwrap();
            journal
                .confirm_stopped(scope(), projection.generation(), id)
                .unwrap();
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
#[test]
fn ignored_task_retirement_writes_roll_back_partial_deletion() {
    let mut failures = Vec::new();
    for (table, operation, condition) in [
        ("task_retirements", "INSERT", ""),
        ("task_records", "DELETE", "WHEN OLD.sequence=0"),
        ("task_blobs", "DELETE", "WHEN OLD.bytes=X'696e707574'"),
        ("task_heads", "DELETE", ""),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("suppressed-retirement.db");
        let mut journal = open(&path);
        let projection = completed(&mut journal);
        journal
            .begin_fence(scope(), projection.generation())
            .unwrap();
        journal
            .confirm_stopped(scope(), projection.generation(), None)
            .unwrap();
        let connection = Connection::open(&path).unwrap();
        let before = snapshot(&connection, TABLES);
        connection.execute_batch(&format!("CREATE TRIGGER suppress BEFORE {operation} ON {table} {condition} BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
        let result = journal
            .retire_task(scope(), projection.next_sequence())
            .map(|_retirement| ());
        let unchanged = snapshot(&connection, TABLES) == before;
        connection.execute_batch("DROP TRIGGER suppress").unwrap();
        if result != Err(TaskPortError::Corrupt) || !unchanged {
            failures.push(format!(
                "{table}/{operation}: {result:?}, unchanged={unchanged}"
            ));
            continue;
        }
        assert_eq!(
            journal
                .retire_task(scope(), projection.next_sequence())
                .unwrap()
                .next_sequence,
            projection.next_sequence()
        );
        for retired_table in ["task_heads", "task_records", "task_blobs"] {
            let remaining: u32 = connection
                .query_row(
                    &format!("SELECT count(*) FROM {retired_table}"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(remaining, 0);
        }
        assert_eq!(
            open(&path).read(scope(), TaskSequence::ZERO),
            Err(TaskPortError::Conflict)
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
