//! Warm statement reuse must still validate current durable timer evidence.
use super::*;

#[test]
fn warmed_timer_operations_reject_changed_evidence_without_writes() {
    for action_row in [false, true] {
        for fault in [
            "checksum",
            "checksum-type",
            "checksum-size",
            "payload-type",
            "payload-size",
            "payload-identity",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("timer-evidence.db");
            let adapter = store(&path, clock());
            let committed = commit(true);
            block_on(ProcessStore::commit(&adapter, &committed)).unwrap();
            let timer = TimerSchedule {
                action: committed.actions[0].clone(),
                due_at: LogicalTime(100),
            };
            block_on(adapter.schedule(&timer)).unwrap();
            // Both SELECT statements now have been reused with present records.
            block_on(adapter.schedule(&timer)).unwrap();
            let connection = Connection::open(&path).unwrap();
            let (table, column) = if action_row {
                ("process_actions", "action")
            } else {
                ("process_timers", "timer")
            };
            let expected = match fault {
                "checksum" => {
                    connection
                        .execute(&format!("UPDATE {table} SET checksum=zeroblob(32)"), [])
                        .unwrap();
                    PortError::Invariant
                }
                "payload-identity" => {
                    let mut changed = timer.clone();
                    changed.action.payload_digest = ContentDigest([7; 32]);
                    let bytes = if action_row {
                        serde_json::to_vec(&changed.action).unwrap()
                    } else {
                        serde_json::to_vec(&changed).unwrap()
                    };
                    connection
                        .execute(
                            &format!("UPDATE {table} SET {column}=?1,checksum=?2"),
                            rusqlite::params![bytes, ContentDigest::sha256(&bytes).0.as_slice()],
                        )
                        .unwrap();
                    PortError::Conflict
                }
                _ => {
                    let field = if fault.starts_with("checksum") {
                        "checksum"
                    } else {
                        column
                    };
                    let value = if fault.ends_with("type") {
                        "'wrong SQLite storage class'".to_owned()
                    } else {
                        let size = if field == "checksum" {
                            33
                        } else {
                            u64::from(SqliteLimits::default().max_record_bytes) + 1
                        };
                        format!("zeroblob({size})")
                    };
                    connection
                        .execute(&format!("UPDATE {table} SET {field}={value}"), [])
                        .unwrap();
                    PortError::Invariant
                }
            };
            let tables = [
                "process_usage",
                "process_actions",
                "process_timers",
                "lease_counter",
            ];
            let before = write_snapshot::snapshot(&connection, &tables);
            assert_eq!(
                block_on(adapter.schedule(&timer)),
                Err(expected),
                "action_row={action_row} fault={fault}"
            );
            assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
            if !action_row {
                assert_eq!(
                    block_on(adapter.cancel(&timer)),
                    Err(expected),
                    "cancel fault={fault}"
                );
                assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
            }
            let reopened = store(&path, clock());
            assert_eq!(block_on(reopened.schedule(&timer)), Err(expected));
            if !action_row {
                assert_eq!(block_on(reopened.cancel(&timer)), Err(expected));
            }
            assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
        }
    }
}
