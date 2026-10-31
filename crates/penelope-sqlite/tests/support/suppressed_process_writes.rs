//! A suppressed required write must roll back the whole public operation.
use super::write_snapshot::snapshot;
use super::*;
use penelope_ports::payloads::ProcessPayloadStore;
const TABLES: &[&str] = &[
    "process_usage",
    "process_inbox",
    "process_sources",
    "process_outcomes",
    "process_actions",
    "process_heads",
    "process_commits",
    "process_payloads",
    "process_timers",
    "lease_counter",
];
#[test]
fn ignored_process_commit_writes_cannot_return_partial_success() {
    let mut failures = Vec::new();
    for (table, operation) in [
        ("process_usage", "INSERT"),
        ("process_usage", "UPDATE"),
        ("process_inbox", "INSERT"),
        ("process_sources", "INSERT"),
        ("process_outcomes", "INSERT"),
        ("process_actions", "INSERT"),
        ("process_heads", "INSERT"),
        ("process_commits", "INSERT"),
        ("process_inbox", "UPDATE"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("suppressed-commit.db");
        let adapter = store(&path, clock());
        if operation == "UPDATE" && table == "process_usage" {
            block_on(adapter.put_payload(&scope(), ContentDigest::sha256(b"seed"), b"seed"))
                .unwrap();
        }
        let candidate = commit(false).with_canonical_source_event(id("cev_suppressed"));
        let connection = Connection::open(&path).unwrap();
        let before = snapshot(&connection, TABLES);
        connection.execute_batch(&format!("CREATE TRIGGER suppress BEFORE {operation} ON {table} BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
        let result = block_on(ProcessStore::commit(&adapter, &candidate));
        let unchanged = snapshot(&connection, TABLES) == before;
        connection.execute_batch("DROP TRIGGER suppress").unwrap();
        if result != Err(PortError::Invariant) || !unchanged {
            failures.push(format!(
                "{table}/{operation}: {result:?}, unchanged={unchanged}"
            ));
            continue;
        }
        block_on(ProcessStore::commit(&adapter, &candidate))
            .unwrap()
            .validate_for(&candidate)
            .unwrap();
        assert!(
            block_on(ProcessStore::commit(&adapter, &candidate))
                .unwrap()
                .duplicate_input
        );
        let reopened = store(&path, clock());
        assert_eq!(
            block_on(reopened.read_outcomes(&replay()))
                .unwrap()
                .outcomes,
            candidate.outcomes
        );
        assert_eq!(block_on(reopened.claim(&claim())).unwrap().len(), 1);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
#[test]
fn ignored_process_service_writes_preserve_state_and_recover() {
    let mut failures = Vec::new();
    for (mode, table, operation) in [
        ("inbox", "process_inbox", "INSERT"),
        ("payload", "process_payloads", "INSERT"),
        ("schedule", "process_timers", "INSERT"),
        ("cancel", "process_timers", "UPDATE"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("suppressed-service.db");
        let adapter = store(&path, clock());
        let candidate = commit(true);
        let timer = TimerSchedule {
            action: candidate.actions[0].clone(),
            due_at: LogicalTime(100),
        };
        let input = candidate.input.as_ref().unwrap();
        if mode == "schedule" || mode == "cancel" {
            block_on(ProcessStore::commit(&adapter, &candidate)).unwrap();
        }
        let lease = if mode == "cancel" {
            block_on(adapter.schedule(&timer)).unwrap();
            Some(
                block_on(adapter.claim_due(&timer_claim(100)))
                    .unwrap()
                    .remove(0),
            )
        } else {
            None
        };
        let digest = ContentDigest::sha256(b"payload");
        let perform = || match mode {
            "inbox" => block_on(adapter.accept(input)).map(|_receipt| ()),
            "payload" => {
                block_on(adapter.put_payload(&scope(), digest, b"payload")).map(|_digest| ())
            }
            "schedule" => block_on(adapter.schedule(&timer)),
            _ => block_on(adapter.cancel(&timer)),
        };
        let connection = Connection::open(&path).unwrap();
        let before = snapshot(&connection, TABLES);
        connection.execute_batch(&format!("CREATE TRIGGER suppress BEFORE {operation} ON {table} BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
        let result = perform();
        let unchanged = snapshot(&connection, TABLES) == before;
        connection.execute_batch("DROP TRIGGER suppress").unwrap();
        if result != Err(PortError::Invariant) || !unchanged {
            failures.push(format!("{mode}: {result:?}, unchanged={unchanged}"));
            continue;
        }
        perform().unwrap();
        perform().unwrap();
        let reopened = store(&path, clock());
        match mode {
            "inbox" => assert!(block_on(reopened.accept(input)).unwrap().duplicate),
            "payload" => assert_eq!(
                block_on(reopened.read_payload(&scope(), digest)).unwrap(),
                b"payload"
            ),
            "schedule" => assert_eq!(
                block_on(reopened.claim_due(&timer_claim(100)))
                    .unwrap()
                    .len(),
                1
            ),
            _ => {
                assert!(
                    block_on(reopened.claim_due(&timer_claim(1000)))
                        .unwrap()
                        .is_empty()
                );
                assert_eq!(
                    block_on(TimerClaimStore::acknowledge(
                        &reopened,
                        lease.as_ref().unwrap(),
                        LogicalTime(100)
                    )),
                    Err(PortError::Conflict)
                );
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
