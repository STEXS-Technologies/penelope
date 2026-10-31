//! End-of-log reads must not conceal a damaged durable frontier.
use super::*;

#[test]
fn reads_reject_inconsistent_frontiers_without_mutation() {
    let damage = [
        "DELETE FROM task_heads",
        "UPDATE task_heads SET next_sequence=1",
        "UPDATE task_heads SET next_sequence=3",
        "DELETE FROM task_records WHERE sequence=1",
        "DELETE FROM task_records",
        "INSERT INTO task_records SELECT scope,2,record,checksum FROM task_records WHERE sequence=1",
        "UPDATE task_heads SET last_digest=x''",
        "UPDATE task_heads SET last_digest='not a blob'",
        "UPDATE task_heads SET last_digest=zeroblob(33)",
    ];
    for sql in damage {
        for cursor in [0, 1, 2, 99] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("frontier.db");
            let mut journal = open(&path);
            committed_start(&mut journal);
            let connection = Connection::open(&path).unwrap();
            let tables = ["task_heads", "task_records", "task_blobs"];
            let healthy = write_snapshot::snapshot(&connection, &tables);
            connection.execute_batch("BEGIN IMMEDIATE").unwrap();
            // Simulate persisted damage, including values normally rejected by
            // schema CHECK constraints; restore normal enforcement afterwards.
            connection
                .execute_batch("PRAGMA ignore_check_constraints=ON")
                .unwrap();
            connection.execute(sql, []).unwrap();
            connection
                .execute_batch("PRAGMA ignore_check_constraints=OFF")
                .unwrap();
            connection.execute_batch("COMMIT").unwrap();
            let damaged = write_snapshot::snapshot(&connection, &tables);
            assert_eq!(
                journal.read(scope(), TaskSequence::new(cursor)),
                Err(TaskPortError::Corrupt),
                "damage={sql}, cursor={cursor}"
            );
            assert_eq!(write_snapshot::snapshot(&connection, &tables), damaged);
            // Restore the authoritative fixture, then reuse the same connection
            // and cached statement before checking a reopened journal.
            connection.execute("DELETE FROM task_heads", []).unwrap();
            connection.execute("DELETE FROM task_records", []).unwrap();
            for (table, rows) in healthy.iter().take(2) {
                for row in rows {
                    let placeholders = vec!["?"; row.len()].join(",");
                    connection
                        .execute(
                            &format!("INSERT INTO {table} VALUES ({placeholders})"),
                            rusqlite::params_from_iter(row),
                        )
                        .unwrap();
                }
            }
            assert_eq!(write_snapshot::snapshot(&connection, &tables), healthy);
            assert_eq!(
                journal.read(scope(), TaskSequence::ZERO).unwrap(),
                Some(intent())
            );
            assert_eq!(journal.read(scope(), TaskSequence::new(2)).unwrap(), None);
            drop(journal);
            assert_eq!(
                open(&path).read(scope(), TaskSequence::new(2)).unwrap(),
                None
            );
        }
    }
}

#[test]
fn cached_frontier_query_rebinds_scope_and_observes_new_tail() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("healthy-frontier.db");
    let mut reader = open(&path);
    let mut other = scope();
    other.task = TaskId::from_digest(Digest([88; 32]));
    for requested in [scope(), other, scope()] {
        assert_eq!(reader.read(requested, TaskSequence::ZERO).unwrap(), None);
    }
    let mut writer = open(&path);
    committed_start(&mut writer);
    for cursor in [0, 1, 2, 99] {
        assert_eq!(reader.read(other, TaskSequence::new(cursor)).unwrap(), None);
        let record = reader.read(scope(), TaskSequence::new(cursor)).unwrap();
        assert_eq!(record.is_some(), cursor < 2);
        if cursor == 0 {
            assert_eq!(record, Some(intent()));
        }
    }
}
