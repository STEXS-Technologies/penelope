use super::*;
use penelope_ports::inbox::{MAX_PENDING_INPUT_BATCH, PendingInbox};

#[test]
fn discovery_is_bounded_scoped_repeatable_and_only_atomic_commit_consumes_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inbox.db");
    let mut adapter = store(&path, clock());
    let mut commit = commit(false);
    let mut first = commit.input.as_ref().unwrap().clone();
    first.input_id = id("inp_a");
    let mut second = first.clone();
    second.input_id = id("inp_b");
    // Deliberately accept in reverse order: discovery promises ID order, not FIFO.
    block_on(adapter.accept(&second)).unwrap();
    block_on(adapter.accept(&first)).unwrap();
    let scope = scope();
    let limit = NonZeroU16::new(2).unwrap();
    let pending =
        || block_on(adapter.pending_inputs(&scope.tenant_id, &scope.process_id, limit)).unwrap();
    assert_eq!(pending(), vec![first.clone(), second.clone()]);
    assert_eq!(pending(), vec![first.clone(), second.clone()]);
    assert_eq!(
        block_on(adapter.pending_inputs(
            &scope.tenant_id,
            &scope.process_id,
            NonZeroU16::new(1).unwrap()
        ))
        .unwrap(),
        vec![first.clone()]
    );
    assert!(
        block_on(adapter.pending_inputs(&id("tnt_foreign"), &scope.process_id, limit))
            .unwrap()
            .is_empty()
    );
    assert!(
        block_on(adapter.pending_inputs(&scope.tenant_id, &id("prc_foreign"), limit))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        block_on(adapter.pending_inputs(
            &scope.tenant_id,
            &scope.process_id,
            NonZeroU16::new(MAX_PENDING_INPUT_BATCH + 1).unwrap()
        )),
        Err(PortError::QuotaExceeded)
    );
    adapter = store(&path, clock());
    assert_eq!(
        block_on(adapter.pending_inputs(&scope.tenant_id, &scope.process_id, limit)).unwrap(),
        vec![first.clone(), second.clone()]
    );
    commit.input = Some(first.clone());
    commit.outcomes.first_mut().unwrap().causation_id = CausationId::Input(first.input_id.clone());
    block_on(ProcessStore::commit(&adapter, &commit))
        .unwrap()
        .validate_for(&commit)
        .unwrap();
    // A lost commit receipt can be retried without rediscovering consumed work.
    assert!(
        block_on(ProcessStore::commit(&adapter, &commit))
            .unwrap()
            .duplicate_input
    );
    assert!(block_on(adapter.accept(&first)).unwrap().duplicate);
    assert_eq!(
        block_on(adapter.pending_inputs(&scope.tenant_id, &scope.process_id, limit)).unwrap(),
        vec![second]
    );
}

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
fn discovery_rejects_corrupt_misindexed_and_oversized_inputs_without_consuming_them() {
    for case in [
        "checksum",
        "identity",
        "payload-size",
        "tenant",
        "checksum-type",
        "checksum-size",
        "payload-type",
        "malformed",
        "process",
        "checksum-short",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("inbox.db");
        let adapter = store(&path, clock());
        let mut first = commit(false).input.unwrap();
        first.input_id = id("inp_a");
        let mut input = first.clone();
        input.input_id = id("inp_b");
        block_on(adapter.accept(&first)).unwrap();
        block_on(adapter.accept(&input)).unwrap();
        let scope = scope();
        let limit = NonZeroU16::new(16).unwrap();
        assert_eq!(
            block_on(adapter.pending_inputs(&scope.tenant_id, &scope.process_id, limit)).unwrap(),
            vec![first, input.clone()]
        );
        let connection = Connection::open(&path).unwrap();
        let update = match case {
            "checksum" => "checksum=zeroblob(32)",
            "identity" => "input_id='inp_z'",

            "checksum-type" => "checksum='wrong storage class'",
            "checksum-size" => "checksum=zeroblob(33)",
            "payload-type" => "input='wrong storage class'",
            "checksum-short" => "checksum=zeroblob(31)",
            _ => "",
        };
        if update.is_empty() {
            let bytes = if case == "payload-size" {
                let canonical = serde_json::to_vec(&input).unwrap();
                let mut padded = vec![b' '; 16385 - canonical.len()];
                padded.extend_from_slice(&canonical);
                assert_eq!(
                    serde_json::from_slice::<ProcessInput>(&padded).unwrap(),
                    input
                );
                padded
            } else if case == "malformed" {
                b"{malformed".to_vec()
            } else {
                let mut substituted = input.clone();
                if case == "tenant" {
                    substituted.tenant_id = id("tnt_foreign");
                } else {
                    substituted.process_id = id("prc_foreign");
                }
                serde_json::to_vec(&substituted).unwrap()
            };
            connection
                .execute(
                    "UPDATE process_inbox SET input=?1,checksum=?2 WHERE input_id='inp_b'",
                    rusqlite::params![bytes, ContentDigest::sha256(&bytes).0.as_slice()],
                )
                .unwrap();
        } else {
            connection
                .execute(
                    &format!("UPDATE process_inbox SET {update} WHERE input_id='inp_b'"),
                    [],
                )
                .unwrap();
        }
        let before = write_snapshot::snapshot(&connection, &TABLES);
        for reopened in [false, true] {
            let recovered;
            let active = if reopened {
                recovered = store(&path, clock());
                &recovered
            } else {
                &adapter
            };
            assert_eq!(
                block_on(active.pending_inputs(&scope.tenant_id, &scope.process_id, limit)),
                Err(PortError::Invariant),
                "case={case} reopened={reopened}"
            );
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
        }
    }
}
#[test]
fn warmed_discovery_tracks_admission_and_consumption_on_other_workers() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("refresh.db");
    let adapter = store(&path, clock());
    let other = store(&path, clock());
    let scope = scope();
    let limit = NonZeroU16::new(16).unwrap();
    let pending =
        || block_on(adapter.pending_inputs(&scope.tenant_id, &scope.process_id, limit)).unwrap();
    assert!(pending().is_empty());
    let mut first = commit(false);
    first.actions.clear();
    first.input.as_mut().unwrap().input_id = id("inp_a");
    first.outcomes[0].causation_id = CausationId::Input(id("inp_a"));
    let mut second = first.clone();
    second.expected_sequence = 1;
    second.input.as_mut().unwrap().input_id = id("inp_b");
    second.outcomes[0].sequence = 1;
    second.outcomes[0].outcome_id = id("out_b");
    second.outcomes[0].causation_id = CausationId::Input(id("inp_b"));
    let input_a = first.input.as_ref().unwrap();
    let input_b = second.input.as_ref().unwrap();
    block_on(other.accept(input_b)).unwrap();
    assert_eq!(pending(), vec![input_b.clone()]);
    block_on(other.accept(input_a)).unwrap();
    assert_eq!(pending(), vec![input_a.clone(), input_b.clone()]);
    block_on(ProcessStore::commit(&other, &first))
        .unwrap()
        .validate_for(&first)
        .unwrap();
    assert_eq!(pending(), vec![input_b.clone()]);
    let reopened = store(&path, clock());
    assert_eq!(
        block_on(reopened.pending_inputs(&scope.tenant_id, &scope.process_id, limit)).unwrap(),
        vec![input_b.clone()]
    );
    block_on(ProcessStore::commit(&other, &second))
        .unwrap()
        .validate_for(&second)
        .unwrap();
    assert!(pending().is_empty());
    assert!(
        block_on(reopened.pending_inputs(&scope.tenant_id, &scope.process_id, limit))
            .unwrap()
            .is_empty()
    );
}
