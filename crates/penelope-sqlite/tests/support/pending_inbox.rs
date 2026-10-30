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

#[test]
fn discovery_rejects_corrupt_misindexed_and_oversized_inputs_without_consuming_them() {
    for case in 0..4 {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("inbox.db");
        let adapter = store(&path, clock());
        let input = commit(false).input.unwrap();
        block_on(adapter.accept(&input)).unwrap();
        let connection = Connection::open(&path).unwrap();
        match case {
            0 => {
                connection
                    .execute("UPDATE process_inbox SET checksum=zeroblob(32)", [])
                    .unwrap();
            }
            1 => {
                connection
                    .execute("UPDATE process_inbox SET input_id='inp_substituted'", [])
                    .unwrap();
            }
            2 => {
                connection
                    .execute("UPDATE process_inbox SET input=zeroblob(16385)", [])
                    .unwrap();
            }
            _ => {
                let mut substituted = input.clone();
                substituted.tenant_id = id("tnt_foreign");
                let encoded = serde_json::to_vec(&substituted).unwrap();
                let checksum = penelope_core::task::Digest::of(&encoded);
                connection
                    .execute(
                        "UPDATE process_inbox SET input=?1,checksum=?2",
                        rusqlite::params![encoded, checksum.0.as_slice()],
                    )
                    .unwrap();
            }
        }
        let scope = scope();
        assert!(
            block_on(adapter.pending_inputs(
                &scope.tenant_id,
                &scope.process_id,
                NonZeroU16::new(1).unwrap()
            ))
            .is_err()
        );
        let pending: bool = connection
            .query_row(
                "SELECT commit_digest IS NULL FROM process_inbox",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(pending);
    }
}
