//! Cached SQL must rebind scopes and observe external changes on each use.
use super::*;
#[test]
fn statement_reuse_keeps_scope_bindings_and_observes_delete_repair_and_schema_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("statement-scopes.db");
    let mut journal = open(&path);
    let mut second = scope();
    second.task = TaskId::from_digest(Digest([8; 32]));
    let input = BlobRef {
        digest: Digest::of(b"input"),
        bytes: 5,
        schema: 1,
    };
    for identity in [scope(), second] {
        let record = TaskProjection::new(identity, definition())
            .unwrap()
            .propose(
                TaskExecutionEvent::IntentCommitted {
                    definition: definition(),
                    input,
                },
                TaskGeneration::ZERO,
                TaskTick::ZERO,
            )
            .unwrap();
        journal.commit(&record, Some(b"input")).unwrap();
        journal.rebuild_head(identity).unwrap();
        assert_eq!(journal.read(identity, TaskSequence::ZERO), Ok(Some(record)));
    }
    let connection = Connection::open(&path).unwrap();
    let first_key: Vec<u8> = connection
        .query_row(
            "SELECT scope FROM task_blobs ORDER BY scope LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for round in 0..8 {
        // Both scopes share the same digest. A cached digest-only result or a
        // stale scope binding could incorrectly hide the missing first blob.
        for identity in [second, scope()] {
            let mut buffer = [0; 5];
            assert_eq!(journal.read_blob(identity, input, &mut buffer), Ok(5));
            assert_eq!(&buffer, b"input");
            journal.rebuild_head(identity).unwrap();
        }
        connection
            .execute("DELETE FROM task_blobs WHERE scope=?1", [&first_key])
            .unwrap();
        let tables = ["task_heads", "task_records", "task_blobs"];
        let damaged = write_snapshot::snapshot(&connection, &tables);
        let mut buffer = [0; 5];
        assert_eq!(
            journal.read_blob(scope(), input, &mut buffer),
            Err(TaskPortError::Corrupt)
        );
        assert_eq!(journal.rebuild_head(scope()), Err(TaskPortError::Corrupt));
        assert_eq!(write_snapshot::snapshot(&connection, &tables), damaged);
        assert_eq!(journal.read_blob(second, input, &mut buffer), Ok(5));
        assert_eq!(&buffer, b"input");
        journal.rebuild_head(second).unwrap();
        connection
            .execute(
                "INSERT INTO task_blobs SELECT ?1,digest,bytes FROM task_blobs WHERE scope<>?1",
                [&first_key],
            )
            .unwrap();
        connection
            .execute_batch(if round % 2 == 0 {
                "CREATE INDEX task_statement_probe ON task_blobs(digest)"
            } else {
                "DROP INDEX task_statement_probe"
            })
            .unwrap();
        assert_eq!(journal.read_blob(scope(), input, &mut buffer), Ok(5));
        assert_eq!(&buffer, b"input");
        journal.rebuild_head(scope()).unwrap();
        println!(
            "round={round} missing_rejected=true healthy_scope=true repaired=true schema_rebound=true"
        );
    }
    drop(journal);
    let mut reopened = open(&path);
    reopened.rebuild_head(second).unwrap();
    reopened.rebuild_head(scope()).unwrap();
}
