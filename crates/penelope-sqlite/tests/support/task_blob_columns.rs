//! Blob validation must finish before caller buffers or durable rows change.
use super::*;
use rusqlite::types::Value;
#[test]
fn blob_read_rebuild_and_duplicate_commit_preserve_validation() {
    for (name, value, valid) in [
        ("valid", Some(Value::Blob(b"input".to_vec())), true),
        ("wrong", Some(Value::Blob(b"wrong".to_vec())), false),
        ("empty", Some(Value::Blob(Vec::new())), false),
        ("short", Some(Value::Blob(b"inpu".to_vec())), false),
        ("long", Some(Value::Blob(b"input!".to_vec())), false),
        ("oversized", Some(Value::Blob(vec![7; 65])), false),
        ("text", Some(Value::Text("input".to_owned())), false),
        ("integer", Some(Value::Integer(5)), false),
        ("real", Some(Value::Real(5.5)), false),
        ("missing", None, false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("blob-columns.db");
        let limits = SqliteLimits {
            max_blob_bytes: 64,
            ..SqliteLimits::default()
        };
        let mut journal = SqliteTaskJournal::open(&path, definition(), limits).unwrap();
        let mut projection = committed_start(&mut journal);
        let reference = BlobRef {
            digest: Digest::of(b"input"),
            bytes: 5,
            schema: 1,
        };
        let checkpoint = projection
            .propose(
                TaskExecutionEvent::CheckpointCommitted {
                    checkpoint: reference,
                },
                projection.generation(),
                TaskTick::new(2),
            )
            .unwrap();
        let connection = Connection::open(&path).unwrap();
        if let Some(value) = value {
            connection
                .execute("UPDATE task_blobs SET bytes=?1", [value])
                .unwrap();
        } else {
            connection.execute("DELETE FROM task_blobs", []).unwrap();
        }
        let tables = ["task_records", "task_heads", "task_blobs"];
        let before = write_snapshot::snapshot(&connection, &tables);
        // Even if the stored row also contains "wrong", equal buffers must not
        // bypass the caller's digest verification.
        assert_eq!(
            journal.commit(&checkpoint, Some(b"wrong")),
            Err(TaskPortError::Corrupt),
            "unverified caller case={name}"
        );
        assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
        let mut buffer = [0xa5; 16];
        let result = journal.read_blob(scope(), reference, &mut buffer);
        if valid {
            assert_eq!(result, Ok(5), "case={name}");
            assert_eq!(&buffer[..5], b"input");
            assert_eq!(&buffer[5..], &[0xa5; 11]);
        } else {
            assert_eq!(result, Err(TaskPortError::Corrupt), "case={name}");
            assert_eq!(buffer, [0xa5; 16]);
        }
        assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
        if !valid {
            assert_eq!(
                journal.rebuild_head(scope()),
                Err(TaskPortError::Corrupt),
                "rebuild case={name}"
            );
            // Missing rows do not enter the duplicate-row comparison branch;
            // their rejection is exercised through read and reconstruction.
            if name != "missing" {
                assert_eq!(
                    journal.commit(&checkpoint, Some(b"input")),
                    Err(TaskPortError::Corrupt),
                    "commit case={name}"
                );
            }
            assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
        }
        connection.execute("DELETE FROM task_blobs", []).unwrap();
        connection
            .execute(
                "INSERT INTO task_blobs VALUES (?1,?2,?3)",
                rusqlite::params![
                    [
                        scope().tenant.digest().0,
                        scope().task.digest().0,
                        scope().definition.digest().0,
                        scope().input.digest().0
                    ]
                    .concat(),
                    reference.digest.0.as_slice(),
                    b"input"
                ],
            )
            .unwrap();
        journal.rebuild_head(scope()).unwrap();
        assert_eq!(
            journal.commit(&checkpoint, Some(b"input")).unwrap(),
            checkpoint
        );
        projection.apply(&checkpoint).unwrap();
        assert_eq!(projection.checkpoint(), Some(reference));
        assert_eq!(journal.read_blob(scope(), reference, &mut buffer), Ok(5));
        let mut too_small = [0xa5; 4];
        assert_eq!(
            journal.read_blob(scope(), reference, &mut too_small),
            Err(TaskPortError::Exhausted)
        );
        assert_eq!(too_small, [0xa5; 4]);
        drop(journal);
        assert_eq!(
            open(&path).read_blob(scope(), reference, &mut buffer),
            Ok(5)
        );
    }
}

#[test]
fn empty_and_maximum_blob_buffers_survive_corruption_repair_and_reopen() {
    for size in [0_usize, 1048576] {
        let bytes = vec![7; size];
        let mut definition = definition();
        definition.budget.max_input_bytes = 1048576;
        definition.recovery.max_checkpoint_bytes = 1048576;
        let mut scope = scope();
        scope.definition = definition.fingerprint();
        scope.input = TaskInputDigest::from_digest(Digest::of(&bytes));
        let reference = BlobRef {
            digest: Digest::of(&bytes),
            bytes: u32::try_from(size).unwrap(),
            schema: 1,
        };
        let record = TaskProjection::new(scope, definition)
            .unwrap()
            .propose(
                TaskExecutionEvent::IntentCommitted {
                    definition,
                    input: reference,
                },
                TaskGeneration::ZERO,
                TaskTick::ZERO,
            )
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("blob-boundary.db");
        let mut journal =
            SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
        journal.commit(&record, Some(&bytes)).unwrap();
        let mut projection = TaskProjection::new(scope, definition).unwrap();
        projection.apply(&record).unwrap();
        let started = projection
            .propose(
                TaskExecutionEvent::Started,
                TaskGeneration::new(1),
                TaskTick::new(1),
            )
            .unwrap();
        journal.commit(&started, None).unwrap();
        projection.apply(&started).unwrap();
        let checkpoint = projection
            .propose(
                TaskExecutionEvent::CheckpointCommitted {
                    checkpoint: reference,
                },
                projection.generation(),
                TaskTick::new(2),
            )
            .unwrap();
        assert_eq!(journal.commit(&checkpoint, Some(&bytes)), Ok(checkpoint));
        projection.apply(&checkpoint).unwrap();
        let next = projection
            .propose(
                TaskExecutionEvent::CheckpointCommitted {
                    checkpoint: reference,
                },
                projection.generation(),
                TaskTick::new(3),
            )
            .unwrap();
        let mut buffer = vec![0xa5; size + 7];
        assert_eq!(journal.read_blob(scope, reference, &mut buffer), Ok(size));
        assert_eq!(&buffer[..size], bytes);
        assert_eq!(&buffer[size..], &[0xa5; 7]);
        let held = buffer.clone();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("UPDATE task_blobs SET bytes=?1", [vec![8; size.max(1)]])
            .unwrap();
        let tables = ["task_records", "task_heads", "task_blobs"];
        let before = write_snapshot::snapshot(&connection, &tables);
        assert_eq!(
            journal.commit(&next, Some(&bytes)),
            Err(TaskPortError::Corrupt)
        );
        assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
        buffer.fill(0xa5);
        assert_eq!(
            journal.read_blob(scope, reference, &mut buffer),
            Err(TaskPortError::Corrupt)
        );
        assert!(buffer.iter().all(|b| *b == 0xa5));
        assert_eq!(journal.rebuild_head(scope), Err(TaskPortError::Corrupt));
        connection
            .execute("UPDATE task_blobs SET bytes=?1", [&bytes])
            .unwrap();
        journal.rebuild_head(scope).unwrap();
        assert_eq!(journal.commit(&next, Some(&bytes)), Ok(next));
        assert_eq!(journal.read_blob(scope, reference, &mut buffer), Ok(size));
        assert_eq!(buffer, held);
        drop(journal);
        let mut reopened =
            SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
        assert_eq!(reopened.read_blob(scope, reference, &mut buffer), Ok(size));
        assert_eq!(buffer, held);
    }
}
