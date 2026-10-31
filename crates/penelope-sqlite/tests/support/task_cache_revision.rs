//! External changes invalidate cached history even when the head is unchanged.
use super::*;
#[test]
fn cached_appends_reject_external_history_damage_before_writing() {
    let mut unexpected = Vec::new();
    for mode in ["warm", "rebuilt", "cold"] {
        for reuse in [false, true] {
            for damage in [
                "DELETE FROM task_blobs WHERE bytes=x'696e707574'",
                "DELETE FROM task_blobs WHERE bytes=x'7374617465'",
                "UPDATE task_blobs SET bytes=x'77726f6e67' WHERE bytes=x'696e707574'",
                "UPDATE task_blobs SET bytes=x'77726f6e67' WHERE bytes=x'7374617465'",
                "DELETE FROM task_records WHERE sequence=1",
                "UPDATE task_records SET checksum=zeroblob(32) WHERE sequence=0",
                "UPDATE task_records SET record=zeroblob(length(record)) WHERE sequence=0",
                "INSERT INTO task_blobs SELECT scope,zeroblob(32),x'6f727068616e' FROM task_heads",
            ] {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("cache-revision.db");
                let mut journal = open(&path);
                let mut projection = committed_start(&mut journal);
                for (tick, bytes) in [(2, b"state".as_slice()), (3, b"new".as_slice())] {
                    let record = projection
                        .propose(
                            TaskExecutionEvent::CheckpointCommitted {
                                checkpoint: BlobRef {
                                    digest: Digest::of(bytes),
                                    bytes: u32::try_from(bytes.len()).unwrap(),
                                    schema: 1,
                                },
                            },
                            projection.generation(),
                            TaskTick::new(tick),
                        )
                        .unwrap();
                    journal.commit(&record, Some(bytes)).unwrap();
                    projection.apply(&record).unwrap();
                }
                let bytes = if reuse {
                    b"input".as_slice()
                } else {
                    b"done".as_slice()
                };
                let reference = BlobRef {
                    digest: Digest::of(bytes),
                    bytes: u32::try_from(bytes.len()).unwrap(),
                    schema: 1,
                };
                let event = if reuse {
                    TaskExecutionEvent::CheckpointCommitted {
                        checkpoint: reference,
                    }
                } else {
                    TaskExecutionEvent::Completed { output: reference }
                };
                let record = projection
                    .propose(event, projection.generation(), TaskTick::new(4))
                    .unwrap();
                if mode == "rebuilt" {
                    journal.rebuild_head(scope()).unwrap();
                }
                if mode == "cold" {
                    drop(journal);
                    journal = open(&path);
                }
                let connection = Connection::open(&path).unwrap();
                let tables = [
                    "task_heads",
                    "task_records",
                    "task_blobs",
                    "task_retirements",
                    "runtime_generations",
                ];
                let healthy = write_snapshot::snapshot(&connection, &tables);
                connection.execute(damage, []).unwrap();
                let damaged = write_snapshot::snapshot(&connection, &tables);
                let result = journal.commit(&record, Some(bytes));
                let unchanged = write_snapshot::snapshot(&connection, &tables) == damaged;
                println!(
                    "mode={mode} reuse={reuse} damage={damage} accepted={} unchanged={unchanged}",
                    result.is_ok()
                );
                if result != Err(TaskPortError::Corrupt) || !unchanged {
                    unexpected.push(format!("mode={mode} reuse={reuse} damage={damage} result={result:?} unchanged={unchanged}"));
                }
                connection.execute_batch("BEGIN IMMEDIATE").unwrap();
                for (table, rows) in &healthy {
                    connection
                        .execute(&format!("DELETE FROM {table}"), [])
                        .unwrap();
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
                connection.execute_batch("COMMIT").unwrap();
                assert_eq!(journal.commit(&record, Some(bytes)), Ok(record));
                journal.rebuild_head(scope()).unwrap();
                drop(journal);
                let mut reopened = open(&path);
                assert_eq!(
                    reopened.read(scope(), TaskSequence::new(4)),
                    Ok(Some(record))
                );
                let mut buffer = [0; 5];
                assert_eq!(
                    reopened.read_blob(scope(), reference, &mut buffer),
                    Ok(bytes.len())
                );
                assert_eq!(&buffer[..bytes.len()], bytes);
            }
        }
    }
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
}

#[test]
fn external_transactions_rebind_cache_after_lock_failure_commit_and_rollback() {
    for commit_damage in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("external-transaction.db");
        let mut journal = SqliteTaskJournal::open(
            &path,
            definition(),
            SqliteLimits {
                busy_timeout: std::time::Duration::ZERO,
                ..SqliteLimits::default()
            },
        )
        .unwrap();
        let projection = committed_start(&mut journal);
        let completion = projection
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
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("BEGIN IMMEDIATE; DELETE FROM task_blobs")
            .unwrap();
        let tables = ["task_records", "task_heads", "task_blobs"];
        let held = write_snapshot::snapshot(&connection, &tables);
        assert_eq!(
            journal.commit(&completion, Some(b"done")),
            Err(TaskPortError::Unavailable)
        );
        assert_eq!(write_snapshot::snapshot(&connection, &tables), held);
        if commit_damage {
            connection.execute_batch("COMMIT").unwrap();
            assert_eq!(
                journal.commit(&completion, Some(b"done")),
                Err(TaskPortError::Corrupt)
            );
            assert_eq!(write_snapshot::snapshot(&connection, &tables), held);
            connection
                .execute(
                    "INSERT INTO task_blobs SELECT scope,?1,?2 FROM task_heads",
                    rusqlite::params![Digest::of(b"input").0.as_slice(), b"input"],
                )
                .unwrap();
        } else {
            connection.execute_batch("ROLLBACK").unwrap();
        }
        assert_eq!(journal.commit(&completion, Some(b"done")), Ok(completion));
        journal.rebuild_head(scope()).unwrap();
        drop(journal);
        assert_eq!(
            open(&path).read(scope(), TaskSequence::new(2)),
            Ok(Some(completion))
        );
    }
}

#[test]
fn unrelated_scope_damage_and_schema_changes_do_not_poison_healthy_appends() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("unrelated-transaction.db");
    let mut journal = open(&path);
    let mut projection = committed_start(&mut journal);
    let mut other_scope = scope();
    other_scope.task = TaskId::from_digest(Digest([8; 32]));
    other_scope.input = TaskInputDigest::from_digest(Digest::of(b"other"));
    let other = TaskProjection::new(other_scope, definition())
        .unwrap()
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition: definition(),
                input: BlobRef {
                    digest: Digest::of(b"other"),
                    bytes: 5,
                    schema: 1,
                },
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        )
        .unwrap();
    let mut writer = open(&path);
    writer.commit(&other, Some(b"other")).unwrap();
    let connection = Connection::open(&path).unwrap();
    for (tick, sql) in [
        (
            2,
            "UPDATE task_blobs SET bytes=x'77726f6e67' WHERE bytes=x'6f74686572'",
        ),
        (3, "CREATE INDEX cache_revision_probe ON task_blobs(digest)"),
        (4, "DROP INDEX cache_revision_probe"),
    ] {
        connection.execute_batch(sql).unwrap();
        let record = projection
            .propose(
                TaskExecutionEvent::CheckpointCommitted {
                    checkpoint: BlobRef {
                        digest: Digest::of(b"input"),
                        bytes: 5,
                        schema: 1,
                    },
                },
                projection.generation(),
                TaskTick::new(tick),
            )
            .unwrap();
        assert_eq!(journal.commit(&record, Some(b"input")), Ok(record));
        projection.apply(&record).unwrap();
    }
    journal.rebuild_head(scope()).unwrap();
    assert_eq!(
        writer.rebuild_head(other_scope),
        Err(TaskPortError::Corrupt)
    );
    drop(journal);
    let mut reopened = open(&path);
    assert_eq!(
        reopened
            .read(scope(), TaskSequence::new(4))
            .unwrap()
            .unwrap()
            .tick,
        TaskTick::new(4)
    );
    reopened.rebuild_head(scope()).unwrap();
}
