//! Cached projections cannot authorize appends over a missing retained tail.
use super::*;
#[test]
fn appends_reject_damaged_frontiers_for_warm_rebuilt_and_cold_projections() {
    let mut unexpected = Vec::new();
    for mode in ["warm", "rebuilt", "cold"] {
        for damage in [
            "DELETE FROM task_heads",
            "UPDATE task_heads SET next_sequence=1",
            "UPDATE task_heads SET next_sequence=3",
            "DELETE FROM task_records WHERE sequence=1",
            "DELETE FROM task_records",
            "INSERT INTO task_records SELECT scope,2,record,checksum FROM task_records WHERE sequence=1",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("append-frontier.db");
            let mut journal = open(&path);
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
            let result = journal.commit(&completion, Some(b"done"));
            println!(
                "mode={mode} damage={damage} accepted={} unchanged={}",
                result.is_ok(),
                write_snapshot::snapshot(&connection, &tables) == damaged
            );
            if result != Err(TaskPortError::Corrupt) {
                unexpected.push(format!("mode={mode}, damage={damage}, result={result:?}"));
            } else {
                assert_eq!(write_snapshot::snapshot(&connection, &tables), damaged);
            }
            // Replace all rows from authoritative test evidence, including any
            // blob/record an incorrect success might have inserted.
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
            // Both a warm cache left by rejection and a cache left by an
            // incorrect success must observe the repaired durable frontier.
            assert_eq!(
                journal.commit(&completion, Some(b"done")).unwrap(),
                completion
            );
            drop(journal);
            let mut reopened = open(&path);
            assert_eq!(
                reopened.read(scope(), TaskSequence::new(2)).unwrap(),
                Some(completion)
            );
            let output = BlobRef {
                digest: Digest::of(b"done"),
                bytes: 4,
                schema: 1,
            };
            let mut buffer = [0; 4];
            assert_eq!(reopened.read_blob(scope(), output, &mut buffer).unwrap(), 4);
            assert_eq!(&buffer, b"done");
        }
    }
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
}
