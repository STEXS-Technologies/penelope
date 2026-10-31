//! Exact inbox retries read committed evidence without claiming writer admission.
use super::*;
const TABLES: [&str; 10] = [
    "process_usage",
    "process_heads",
    "process_outcomes",
    "process_inbox",
    "process_sources",
    "process_commits",
    "process_actions",
    "process_timers",
    "process_payloads",
    "lease_counter",
];
#[test]
fn pending_and_consumed_retries_preserve_quota_scope_and_committed_state() {
    let mut cases = 0;
    for consumed in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("retry.db");
        let mut adapter = store(&path, clock());
        let commit = commit(false);
        let input = commit.input.as_ref().unwrap();
        if consumed {
            block_on(adapter.commit(&commit)).unwrap();
        } else {
            assert!(!block_on(adapter.accept(input)).unwrap().duplicate);
        }
        let connection = Connection::open(&path).unwrap();
        let used: i64 = connection
            .query_row("SELECT bytes_used FROM process_usage", [], |row| row.get(0))
            .unwrap();
        let limits = SqliteLimits {
            max_scope_bytes: u64::try_from(used).unwrap(),
            busy_timeout: Duration::ZERO,
            ..SqliteLimits::default()
        };
        drop(adapter);
        adapter = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
        for reopen in [false, true] {
            if reopen {
                drop(adapter);
                adapter = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
            }
            let receipt = block_on(adapter.accept(input)).unwrap();
            receipt.validate_for(input).unwrap();
            assert!(receipt.duplicate);
            let before = write_snapshot::snapshot(&connection, &TABLES);
            connection.execute_batch("BEGIN IMMEDIATE").unwrap();
            connection
                .execute("UPDATE process_inbox SET input=zeroblob(1)", [])
                .unwrap();
            let retry = block_on(adapter.accept(input)).unwrap();
            retry.validate_for(input).unwrap();
            assert!(retry.duplicate);
            let mut conflict = input.clone();
            conflict.payload_digest = ContentDigest([8; 32]);
            assert_eq!(
                block_on(adapter.accept(&conflict)),
                Err(PortError::Conflict)
            );
            let mut fresh = input.clone();
            fresh.input_id = id("inp_fresh");
            assert_eq!(
                block_on(adapter.accept(&fresh)),
                Err(PortError::Unavailable)
            );
            let mut other = input.clone();
            other.process_id = id("prc_other");
            assert_eq!(
                block_on(adapter.accept(&other)),
                Err(PortError::Unavailable)
            );
            other = input.clone();
            other.tenant_id = id("tnt_other");
            assert_eq!(
                block_on(adapter.accept(&other)),
                Err(PortError::Unavailable)
            );
            connection.execute_batch("ROLLBACK").unwrap();
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            assert_eq!(
                block_on(adapter.accept(&fresh)),
                Err(PortError::QuotaExceeded)
            );
            assert!(block_on(adapter.accept(input)).unwrap().duplicate);
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            cases += 1;
        }
    }
    assert_eq!(cases, 4);
    println!(
        "inbox_retry_lock_cases=4 pending_and_consumed=true full_quota=true scoped=true committed_snapshot=true no_mutation=true"
    );
}
