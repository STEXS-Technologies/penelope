//! Orphans consume quota even before the first record, scoped to one task.
use super::*;
#[test]
fn first_append_accounts_for_orphans_at_exact_quota_across_warm_and_cold_statements() {
    let first = intent();
    let orphan = b"retained orphan";
    let first_bytes = u64::try_from(serde_json::to_vec(&first).unwrap().len()).unwrap() + 5;
    let retained = first_bytes + u64::try_from(orphan.len()).unwrap();
    let identity = scope();
    let key = [
        identity.tenant.digest().0,
        identity.task.digest().0,
        identity.definition.digest().0,
        identity.input.digest().0,
    ]
    .concat();
    for reused_input in [false, true] {
        let orphan = if reused_input {
            b"input".as_slice()
        } else {
            orphan.as_slice()
        };
        let retained = if reused_input { first_bytes } else { retained };
        for cold in [false, true] {
            for bound in [retained - 1, retained, retained + 1] {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("orphan-quota.db");
                let limits = SqliteLimits {
                    max_scope_bytes: bound,
                    ..SqliteLimits::default()
                };
                let mut journal = SqliteTaskJournal::open(&path, definition(), limits).unwrap();
                let connection = Connection::open(&path).unwrap();
                connection
                    .execute(
                        "INSERT INTO task_blobs VALUES (?1,?2,?3)",
                        rusqlite::params![&key, Digest::of(orphan).0.as_slice(), orphan],
                    )
                    .unwrap();
                // A larger orphan in another scope must not consume this quota.
                connection
                    .execute(
                        "INSERT INTO task_blobs VALUES (?1,?2,?3)",
                        rusqlite::params![
                            [9_u8; 128].as_slice(),
                            Digest::of(&[7; 1024]).0.as_slice(),
                            [7_u8; 1024].as_slice()
                        ],
                    )
                    .unwrap();
                let tables = [
                    "task_heads",
                    "task_records",
                    "task_blobs",
                    "runtime_generations",
                ];
                let before = write_snapshot::snapshot(&connection, &tables);
                assert_eq!(journal.rebuild_head(scope()), Err(TaskPortError::Corrupt));
                assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
                if cold {
                    drop(journal);
                    journal = SqliteTaskJournal::open(&path, definition(), limits).unwrap();
                }
                let mut final_limits = limits;
                if bound < retained {
                    assert_eq!(
                        journal.commit(&first, Some(b"input")),
                        Err(TaskPortError::Exhausted)
                    );
                    assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
                    // Explicitly increasing the quota must allow the exact retry,
                    // preserving existing bytes and charging deduplicated input once.
                    drop(journal);
                    final_limits.max_scope_bytes = retained;
                    journal = SqliteTaskJournal::open(&path, definition(), final_limits).unwrap();
                }
                assert_eq!(journal.commit(&first, Some(b"input")), Ok(first));
                let used: i64 = connection
                    .query_row(
                        "SELECT bytes_used FROM task_heads WHERE scope=?1",
                        [&key],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(u64::try_from(used).unwrap(), retained);
                journal.rebuild_head(scope()).unwrap();
                drop(journal);
                let mut reopened =
                    SqliteTaskJournal::open(&path, definition(), final_limits).unwrap();
                assert_eq!(reopened.read(scope(), TaskSequence::ZERO), Ok(Some(first)));
                reopened.rebuild_head(scope()).unwrap();
                let reopened_used: i64 = connection
                    .query_row(
                        "SELECT bytes_used FROM task_heads WHERE scope=?1",
                        [&key],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(u64::try_from(reopened_used).unwrap(), retained);
                println!(
                    "reused_input={reused_input} cold={cold} bound={bound} retained={retained} rejection_atomic={} exact_usage=true repaired_reopened=true",
                    bound < retained
                );
            }
        }
    }
}
