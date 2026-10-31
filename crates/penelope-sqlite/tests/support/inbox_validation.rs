//! Duplicate admission must preserve JSON compatibility and durable evidence checks.
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
fn duplicate_admission_preserves_encodings_conflicts_and_corruption_rejection() {
    duplicate_cases(false);
}
#[test]
fn duplicate_admission_validates_evidence_without_the_writer_lock() {
    duplicate_cases(true);
}
fn test_store(path: &std::path::Path, writer_locked: bool) -> SqliteProcessStore {
    if writer_locked {
        SqliteProcessStore::open(
            path,
            SqliteLimits {
                busy_timeout: Duration::ZERO,
                ..SqliteLimits::default()
            },
            clock(),
            10,
            16,
        )
        .unwrap()
    } else {
        store(path, clock())
    }
}
fn duplicate_cases(writer_locked: bool) {
    let mut cases = 0;
    for consumed in [false, true] {
        for case in [
            "canonical",
            "pretty",
            "escaped",
            "conflict",
            "checksum",
            "checksum-short",
            "checksum-type",
            "checksum-size",
            "payload-type",
            "payload-size",
            "malformed",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("inbox-evidence.db");
            let adapter = test_store(&path, writer_locked);
            let committed = commit(false);
            let input = committed.input.as_ref().unwrap();
            if consumed {
                block_on(ProcessStore::commit(&adapter, &committed)).unwrap();
            } else {
                assert!(!block_on(adapter.accept(input)).unwrap().duplicate);
            }
            assert!(block_on(adapter.accept(input)).unwrap().duplicate);
            let connection = Connection::open(&path).unwrap();
            let canonical = serde_json::to_vec(input).unwrap();
            let expected = match case {
                "pretty" | "escaped" | "conflict" | "malformed" => {
                    let bytes = match case {
                        "pretty" => serde_json::to_vec_pretty(input).unwrap(),
                        "escaped" => String::from_utf8(canonical.clone())
                            .unwrap()
                            .replace("inp_start", "\\u0069np_start")
                            .into_bytes(),
                        "conflict" => {
                            let mut other = input.clone();
                            other.payload_digest = ContentDigest([7; 32]);
                            serde_json::to_vec(&other).unwrap()
                        }
                        _ => b"{malformed".to_vec(),
                    };
                    if case == "pretty" || case == "escaped" {
                        assert_ne!(bytes, canonical);
                        assert_eq!(
                            serde_json::from_slice::<ProcessInput>(&bytes).unwrap(),
                            *input
                        );
                    }
                    connection
                        .execute(
                            "UPDATE process_inbox SET input=?1,checksum=?2",
                            rusqlite::params![bytes, ContentDigest::sha256(&bytes).0.as_slice()],
                        )
                        .unwrap();
                    match case {
                        "conflict" => Some(PortError::Conflict),
                        "malformed" => Some(PortError::Invariant),
                        _ => None,
                    }
                }
                "canonical" => None,
                _ => {
                    let (field, value) = match case {
                        "checksum" => ("checksum", "zeroblob(32)".to_owned()),
                        "checksum-short" => ("checksum", "zeroblob(31)".to_owned()),
                        "checksum-type" => ("checksum", "'wrong storage class'".to_owned()),
                        "checksum-size" => ("checksum", "zeroblob(33)".to_owned()),
                        "payload-type" => ("input", "'wrong storage class'".to_owned()),
                        _ => (
                            "input",
                            format!(
                                "zeroblob({})",
                                u64::from(SqliteLimits::default().max_record_bytes) + 1
                            ),
                        ),
                    };
                    connection
                        .execute(&format!("UPDATE process_inbox SET {field}={value}"), [])
                        .unwrap();
                    Some(PortError::Invariant)
                }
            };
            let before = write_snapshot::snapshot(&connection, &TABLES);
            for reopened in [false, true] {
                let recovered;
                let active = if reopened {
                    recovered = test_store(&path, writer_locked);
                    &recovered
                } else {
                    &adapter
                };
                if writer_locked {
                    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
                }
                let result = block_on(active.accept(input));
                if writer_locked {
                    connection.execute_batch("ROLLBACK").unwrap();
                }
                cases += 1;
                if let Some(error) = expected {
                    assert_eq!(
                        result,
                        Err(error),
                        "consumed={consumed} case={case} reopened={reopened}"
                    );
                } else {
                    let receipt = result.unwrap();
                    assert!(receipt.duplicate);
                    assert_eq!(receipt.tenant_id, input.tenant_id);
                    assert_eq!(receipt.process_id, input.process_id);
                    assert_eq!(receipt.input_id, input.input_id);
                }
                assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            }
        }
    }
    assert_eq!(cases, 44);
    println!(
        "inbox_retry_validation_cases={cases} writer_locked={writer_locked} pending_and_consumed=true encodings=true no_mutation=true"
    );
}
#[test]
fn stored_record_limit_bounds_noncanonical_duplicate_encodings() {
    for size in [4096_usize, 4097] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("inbox-limit.db");
        let adapter = store(&path, clock());
        let committed = commit(false);
        let input = committed.input.as_ref().unwrap();
        block_on(adapter.accept(input)).unwrap();
        let limits = SqliteLimits {
            max_record_bytes: 4096,
            ..SqliteLimits::default()
        };
        let limited = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
        assert!(block_on(limited.accept(input)).unwrap().duplicate);
        let connection = Connection::open(&path).unwrap();
        let canonical = serde_json::to_vec(input).unwrap();
        let mut bytes = vec![b' '; size - canonical.len()];
        bytes.extend_from_slice(&canonical);
        assert_eq!(
            serde_json::from_slice::<ProcessInput>(&bytes).unwrap(),
            *input
        );
        connection
            .execute(
                "UPDATE process_inbox SET input=?1,checksum=?2",
                rusqlite::params![bytes, ContentDigest::sha256(&bytes).0.as_slice()],
            )
            .unwrap();
        let before = write_snapshot::snapshot(&connection, &TABLES);
        let result = block_on(limited.accept(input));
        if size == 4096 {
            assert!(result.unwrap().duplicate);
        } else {
            assert_eq!(result, Err(PortError::Invariant));
        }
        assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
    }
}
