//! Receipt reconstruction checks semantics and bounds for every constituent kind.
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
fn batch() -> AtomicProcessCommit {
    let mut batch = commit(false).with_canonical_source_event(id("cev_encoding"));
    let mut outcome = batch.outcomes[0].clone();
    outcome.sequence = 1;
    outcome.outcome_id = id("out_second");
    batch.outcomes.push(outcome);
    let mut action = batch.actions[0].clone();
    action.action_id = id("act_second");
    action.step_id = id("stp_second");
    batch.actions.push(action);
    batch
}
fn evidence(component: &str, batch: &AtomicProcessCommit) -> (&'static str, Vec<u8>) {
    match component {
        "outcome" => (
            "UPDATE process_outcomes SET record=?1,checksum=?2 WHERE sequence=1",
            serde_json::to_vec(&batch.outcomes[1]).unwrap(),
        ),
        "action" => (
            "UPDATE process_actions SET action=?1,checksum=?2 WHERE action_id='act_second'",
            serde_json::to_vec(&batch.actions[1]).unwrap(),
        ),
        "input" => (
            "UPDATE process_inbox SET input=?1,checksum=?2",
            serde_json::to_vec(batch.input.as_ref().unwrap()).unwrap(),
        ),
        _ => panic!("unknown component"),
    }
}
fn limited_store(path: &std::path::Path) -> SqliteProcessStore {
    SqliteProcessStore::open(
        path,
        SqliteLimits {
            max_record_bytes: 4096,
            ..SqliteLimits::default()
        },
        clock(),
        10,
        16,
    )
    .unwrap()
}
fn check_retry(
    adapter: &SqliteProcessStore,
    batch: &AtomicProcessCommit,
    accepted: bool,
    context: &str,
) {
    let result = block_on(ProcessStore::commit(adapter, batch));
    if accepted {
        let receipt = result.unwrap_or_else(|error| panic!("{context}: {error:?}"));
        receipt.validate_for(batch).unwrap();
        assert!(receipt.duplicate_input, "{context}");
        assert_eq!(receipt.committed_through_sequence, 1, "{context}");
    } else {
        assert_eq!(result, Err(PortError::Invariant), "{context}");
    }
}
#[test]
fn retries_preserve_noncanonical_constituents_and_reject_changed_evidence() {
    let mut cases = 0;
    for component in ["outcome", "action", "input"] {
        for encoding in [
            "canonical",
            "pretty",
            "escaped",
            "reordered",
            "pretty-substitute",
            "escaped-checksum",
            "malformed",
            "canonical-checksum",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("commit-encoding.db");
            let adapter = limited_store(&path);
            let batch = batch();
            block_on(ProcessStore::commit(&adapter, &batch)).unwrap();
            // Warm every verification statement before changing committed bytes.
            check_retry(&adapter, &batch, true, "warmup");
            let connection = Connection::open(&path).unwrap();
            let (sql, canonical) = evidence(component, &batch);
            let value: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
            let bytes = match encoding {
                "pretty" => serde_json::to_vec_pretty(&value).unwrap(),
                "escaped" | "escaped-checksum" => String::from_utf8(canonical.clone())
                    .unwrap()
                    .replacen("tnt_sqlite", "\\u0074nt_sqlite", 1)
                    .into_bytes(),
                "reordered" => serde_json::to_vec(&value).unwrap(),
                "pretty-substitute" => {
                    let mut changed = value.clone();
                    changed["payload_digest"] =
                        serde_json::to_value(ContentDigest([7; 32])).unwrap();
                    assert_ne!(changed, value);
                    serde_json::to_vec_pretty(&changed).unwrap()
                }
                "malformed" => b"{malformed".to_vec(),
                _ => canonical.clone(),
            };
            let accepted = ["canonical", "pretty", "escaped", "reordered"].contains(&encoding);
            if accepted && encoding != "canonical" {
                assert_ne!(bytes, canonical, "{component}/{encoding}");
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                    value
                );
            }
            let checksum = if encoding.ends_with("checksum") {
                ContentDigest([0; 32])
            } else {
                ContentDigest::sha256(&bytes)
            };
            connection
                .execute(sql, rusqlite::params![bytes, checksum.0.as_slice()])
                .unwrap();
            let before = write_snapshot::snapshot(&connection, &TABLES);
            for reopen in [false, true] {
                let reopened;
                let active = if reopen {
                    reopened = limited_store(&path);
                    &reopened
                } else {
                    &adapter
                };
                check_retry(
                    active,
                    &batch,
                    accepted,
                    &format!("{component}/{encoding}/{reopen}"),
                );
                assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 48);
    println!(
        "commit_retry_encoding_cases={cases} constituent_kinds=3 warmed_and_reopened=true semantic_equality=true checksum_required=true no_mutation=true"
    );
}
#[test]
fn retries_bound_noncanonical_constituents_before_semantic_comparison() {
    let mut cases = 0;
    for component in ["outcome", "action", "input"] {
        for size in [4096_usize, 4097] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("commit-record-bound.db");
            let adapter = limited_store(&path);
            let batch = batch();
            block_on(ProcessStore::commit(&adapter, &batch)).unwrap();
            check_retry(&adapter, &batch, true, "warmup");
            let (sql, canonical) = evidence(component, &batch);
            let mut bytes = vec![b' '; size - canonical.len()];
            bytes.extend_from_slice(&canonical);
            assert_eq!(bytes.len(), size);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                serde_json::from_slice::<serde_json::Value>(&canonical).unwrap()
            );
            let connection = Connection::open(&path).unwrap();
            connection
                .execute(
                    sql,
                    rusqlite::params![bytes, ContentDigest::sha256(&bytes).0.as_slice()],
                )
                .unwrap();
            let before = write_snapshot::snapshot(&connection, &TABLES);
            for reopen in [false, true] {
                let reopened;
                let active = if reopen {
                    reopened = limited_store(&path);
                    &reopened
                } else {
                    &adapter
                };
                check_retry(
                    active,
                    &batch,
                    size == 4096,
                    &format!("{component}/{size}/{reopen}"),
                );
                assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 12);
    println!(
        "commit_retry_bound_cases={cases} constituent_kinds=3 inclusive_bound=true warmed_and_reopened=true no_mutation=true"
    );
}
