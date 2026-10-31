//! Exact payload retries are read-only, current, bounded, and scope-isolated.
use super::*;
use penelope_ports::payloads::ProcessPayloadStore;
use rusqlite::types::Value;
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
fn exact_payload_retries_do_not_require_the_writer_lock() {
    let mut cases = 0;
    for width in [9_usize, 128] {
        for size in [0_usize, 16, 1048576] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("retry.db");
            let suffix = "x".repeat(width - 4);
            let scope = ProcessScope::new(
                id(&format!("tnt_{suffix}")),
                id(&format!("prc_{suffix}")),
                id(&format!("def_{suffix}")),
                id(&format!("dfv_{suffix}")),
                ContentDigest([9; 32]),
            );
            let bytes = vec![7; size];
            let digest = ContentDigest::sha256(&bytes);
            let limits = SqliteLimits {
                busy_timeout: Duration::ZERO,
                max_scope_bytes: u64::try_from(scope.canonical_bytes().len() + size + 128).unwrap(),
                ..SqliteLimits::default()
            };
            let mut adapter = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
            block_on(adapter.put_payload(&scope, digest, &bytes)).unwrap();
            let connection = Connection::open(&path).unwrap();
            for reopen in [false, true] {
                if reopen {
                    drop(adapter);
                    adapter = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
                }
                // Warm the SELECT, then keep an external write transaction open.
                assert_eq!(
                    block_on(adapter.put_payload(&scope, digest, &bytes)),
                    Ok(digest)
                );
                let before = write_snapshot::snapshot(&connection, &TABLES);
                connection.execute_batch("BEGIN IMMEDIATE").unwrap();
                // Uncommitted bytes must never replace committed retry evidence.
                connection
                    .execute(
                        "UPDATE process_payloads SET bytes=?1",
                        [b"uncommitted".as_slice()],
                    )
                    .unwrap();
                assert_eq!(
                    block_on(adapter.put_payload(&scope, digest, &bytes)),
                    Ok(digest),
                    "width={width} size={size} reopen={reopen}"
                );
                let other = ProcessScope {
                    process_id: id("prc_other"),
                    ..scope.clone()
                };
                assert_eq!(
                    block_on(adapter.put_payload(&other, digest, &bytes)),
                    Err(PortError::Unavailable)
                );
                let fresh = b"new payload";
                assert_eq!(
                    block_on(adapter.put_payload(&scope, ContentDigest::sha256(fresh), fresh)),
                    Err(PortError::Unavailable)
                );
                assert_eq!(
                    block_on(adapter.put_payload(&scope, digest, b"forged")),
                    Err(PortError::Invariant)
                );
                connection.execute_batch("ROLLBACK").unwrap();
                assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
                // The full logical quota still permits an exact retry, but not a new row.
                assert_eq!(
                    block_on(adapter.put_payload(&scope, digest, &bytes)),
                    Ok(digest)
                );
                assert_eq!(
                    block_on(adapter.put_payload(&scope, ContentDigest::sha256(fresh), fresh)),
                    Err(PortError::QuotaExceeded)
                );
                assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 12);
    println!(
        "payload_retry_lock_cases=12 full_quota=true warmed_and_reopened=true committed_snapshot=true scope_isolation=true no_mutation=true"
    );
}
#[test]
fn locked_payload_retries_revalidate_committed_corruption() {
    let bytes = vec![7; 16];
    let digest = ContentDigest::sha256(&bytes);
    let mut cases = 0;
    for stored in [
        Value::Blob(vec![8; 16]),
        Value::Blob(vec![]),
        Value::Blob(vec![7; 17]),
        Value::Text("x".repeat(16)),
        Value::Integer(7),
        Value::Real(1.5),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("corrupt.db");
        let limits = SqliteLimits {
            max_blob_bytes: 16,
            busy_timeout: Duration::ZERO,
            ..SqliteLimits::default()
        };
        let mut adapter = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
        block_on(adapter.put_payload(&scope(), digest, &bytes)).unwrap();
        let connection = Connection::open(&path).unwrap();
        // Publish corruption after the adapter has warmed its statement.
        connection
            .execute("UPDATE process_payloads SET bytes=?1", [stored])
            .unwrap();
        for reopen in [false, true] {
            if reopen {
                drop(adapter);
                adapter = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
            }
            let before = write_snapshot::snapshot(&connection, &TABLES);
            connection.execute_batch("BEGIN IMMEDIATE").unwrap();
            assert_eq!(
                block_on(adapter.put_payload(&scope(), digest, &bytes)),
                Err(PortError::Invariant)
            );
            connection.execute_batch("ROLLBACK").unwrap();
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            cases += 1;
        }
    }
    assert_eq!(cases, 12);
    println!(
        "payload_retry_corruption_cases=12 warmed_and_reopened=true locked=true no_mutation=true"
    );
}
