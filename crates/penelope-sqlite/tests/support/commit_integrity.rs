//! A saved commit digest cannot replace verification of its durable constituents.
use super::*;

#[test]
fn retry_rejects_damaged_consumption_and_source_bindings() {
    for damage in [
        "UPDATE process_inbox SET commit_digest=zeroblob(32)",
        "UPDATE process_inbox SET committed_through=99",
        "UPDATE process_inbox SET committed_through=NULL",
        "DELETE FROM process_sources",
        "UPDATE process_actions SET effect_key=zeroblob(32)",
        "UPDATE process_sources SET input_id='inp_other'",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("integrity.db");
        let adapter = store(&path, clock());
        let commit = commit(false).with_canonical_source_event(id("cev_integrity"));
        block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection.execute(damage, []).unwrap();
        assert_eq!(
            block_on(ProcessStore::commit(&adapter, &commit)),
            Err(PortError::Invariant),
            "{damage}"
        );
        // Retrying cannot silently repair or append records under damaged evidence.
        let count: u32 = connection
            .query_row("SELECT count(*) FROM process_outcomes", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}

#[test]
fn receipt_reconstruction_rejects_missing_and_substituted_constituents() {
    for component in ["outcome", "action", "input", "head"] {
        for damage in [
            "delete",
            "substitute",
            "checksum",
            "payload-type",
            "payload-size",
            "checksum-type",
            "checksum-size",
        ] {
            if component == "head" && !["delete", "substitute"].contains(&damage) {
                continue;
            }
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("constituents.db");
            let adapter = store(&path, clock());
            let mut commit = commit(false);
            let mut second = commit.outcomes[0].clone();
            second.sequence = 1;
            second.outcome_id = id("out_second");
            commit.outcomes.push(second);
            let mut second_action = commit.actions[0].clone();
            second_action.action_id = id("act_second");
            second_action.step_id = id("stp_second");
            commit.actions.push(second_action);
            block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
            let receipt = block_on(ProcessStore::commit(&adapter, &commit)).unwrap();
            receipt.validate_for(&commit).unwrap();
            assert!(receipt.duplicate_input);
            let connection = Connection::open(&path).unwrap();
            // Damage the later constituent so earlier statement reuse succeeds.
            let filter = match component {
                "outcome" => " WHERE sequence=1",
                "action" => " WHERE action_id='act_second'",
                _ => "",
            };
            let (table, column, replacement) = match component {
                "outcome" => {
                    let mut value = commit.outcomes[0].clone();
                    value.payload_digest = ContentDigest([7; 32]);
                    (
                        "process_outcomes",
                        "record",
                        serde_json::to_vec(&value).unwrap(),
                    )
                }
                "action" => {
                    let mut value = commit.actions[0].clone();
                    value.payload_digest = ContentDigest([7; 32]);
                    (
                        "process_actions",
                        "action",
                        serde_json::to_vec(&value).unwrap(),
                    )
                }
                "input" => {
                    let mut value = commit.input.as_ref().unwrap().clone();
                    value.payload_digest = ContentDigest([7; 32]);
                    (
                        "process_inbox",
                        "input",
                        serde_json::to_vec(&value).unwrap(),
                    )
                }
                _ => ("process_heads", "", Vec::new()),
            };
            if damage == "delete" {
                connection
                    .execute(&format!("DELETE FROM {table}{filter}"), [])
                    .unwrap();
            } else if component == "head" {
                connection
                    .execute("UPDATE process_heads SET next_sequence=1", [])
                    .unwrap();
            } else if damage == "checksum" {
                connection
                    .execute(
                        &format!("UPDATE {table} SET checksum=zeroblob(32){filter}"),
                        [],
                    )
                    .unwrap();
            } else if damage.ends_with("type") || damage.ends_with("size") {
                let field = if damage.starts_with("checksum") {
                    "checksum"
                } else {
                    column
                };
                let value = if damage.ends_with("type") {
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
                    .execute(&format!("UPDATE {table} SET {field}={value}{filter}"), [])
                    .unwrap();
            } else {
                // Valid JSON and a matching checksum still cannot replace the
                // immutable constituents of an already committed operation.
                connection
                    .execute(
                        &format!("UPDATE {table} SET {column}=?1,checksum=?2{filter}"),
                        rusqlite::params![
                            replacement,
                            ContentDigest::sha256(&replacement).0.as_slice()
                        ],
                    )
                    .unwrap();
            }
            let tables = [
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
            let before = write_snapshot::snapshot(&connection, &tables);
            assert_eq!(
                block_on(ProcessStore::commit(&adapter, &commit)),
                Err(PortError::Invariant),
                "{component}/{damage}"
            );
            assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
            let reopened = store(&path, clock());
            assert_eq!(
                block_on(ProcessStore::commit(&reopened, &commit)),
                Err(PortError::Invariant)
            );
            assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
        }
    }
}

#[test]
fn later_commits_and_delivery_ack_do_not_invalidate_an_exact_original_retry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("later.db");
    let adapter = store(&path, clock());
    let first = commit(false).with_canonical_source_event(id("cev_first"));
    block_on(ProcessStore::commit(&adapter, &first)).unwrap();
    let lease = block_on(adapter.claim(&claim())).unwrap().remove(0);
    block_on(OutboxStore::acknowledge(&adapter, &lease, LogicalTime(100))).unwrap();
    let mut second = first.clone();
    second.expected_sequence = 1;
    second.input.as_mut().unwrap().input_id = id("inp_second");
    second.outcomes[0].sequence = 1;
    second.outcomes[0].outcome_id = id("out_second");
    second.outcomes[0].causation_id = CausationId::Input(id("inp_second"));
    second.canonical_source_event_id = Some(id("cev_second"));
    second.actions.clear();
    block_on(ProcessStore::commit(&adapter, &second)).unwrap();
    drop(adapter);
    let recovered = store(&path, clock());
    let receipt = block_on(ProcessStore::commit(&recovered, &first)).unwrap();
    receipt.validate_for(&first).unwrap();
    assert!(receipt.duplicate_input);
    assert!(block_on(recovered.claim(&claim())).unwrap().is_empty());
    assert_eq!(
        block_on(recovered.read_outcomes(&replay()))
            .unwrap()
            .outcomes
            .len(),
        2
    );
}
