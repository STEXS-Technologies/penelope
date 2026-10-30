//! Real signed canonical trigger → complete saga → real protected worker →
//! durable result replay. External effect reconciliation remains an explicitly
//! trusted fixture in the shared saga contract; the signed event is the trigger,
//! not an assertion that this worker performed the source's transfer.
use super::*;

fn attach_source(
    store: &SqliteProcessStore,
    scope: &ProcessScope,
    mut batch: AtomicProcessCommit,
) -> AtomicProcessCommit {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("canonical.db");
    let mut source = Connection::open(&path).unwrap();
    source.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE committed_source(id INTEGER PRIMARY KEY,signed BLOB NOT NULL,events BLOB NOT NULL)").unwrap();
    let event = event();
    let key = SigningKey::from_bytes(&[7; 32]);
    let signed = signed(&event, &key);
    write_source(&mut source, &signed, &[event.clone()]).unwrap();
    drop(source);
    // Use an independently opened reader after the source transaction commits.
    let reader = Connection::open(&path).unwrap();
    let canonical = verified_source(&reader, &key).unwrap();
    let expected = CanonicalCommandExpectation {
        scope: scope.clone(),
        action_id: id("act_source"),
        operation: id("op_transfer"),
        resource_ids: vec![id("res_source")],
        expected_event_payload_digest: ContentDigest::sha256(&canonicalize(&event).unwrap()),
    };
    let verified = verify_committed_event(&expected, canonical).unwrap();
    // Engine event bytes remain the authoritative replay payload on outcomes.
    // The canonical input separately binds the verified upstream event bytes.
    batch.input.as_mut().unwrap().payload_digest = verified.event.payload_digest;
    run(store.put_payload(
        scope,
        verified.event.payload_digest,
        &canonicalize(&event).unwrap(),
    ))
    .unwrap();
    let batch = bind_verified_event_to_commit(batch, &verified).unwrap();
    assert!(batch.canonical_source_event_id.is_some());
    batch
}
fn scope() -> ProcessScope {
    ProcessScope::new(
        id("tnt_source"),
        id("prc_signed_saga"),
        id("def_signed_saga"),
        id("dfv_one"),
        ContentDigest([9; 32]),
    )
}
#[test]
fn signed_source_drives_pure_linear_and_graph_sagas_in_both_store_layouts() {
    sagas::run_linear_saga_with_source(false, &scope(), attach_source);
    sagas::run_graph_saga_with_source(false, &scope(), attach_source);
}
#[test]
fn signed_source_drives_reconciled_linear_and_graph_sagas_in_both_store_layouts() {
    sagas::run_linear_saga_with_source(true, &scope(), attach_source);
    sagas::run_graph_saga_with_source(true, &scope(), attach_source);
}
