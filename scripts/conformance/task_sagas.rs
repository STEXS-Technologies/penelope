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
    let temporary = std::env::var_os("PENELOPE_SAGA_CRASH_DIR")
        .is_none()
        .then(|| tempfile::tempdir().unwrap());
    let directory = temporary.as_ref().map_or_else(
        || std::path::PathBuf::from(std::env::var_os("PENELOPE_SAGA_CRASH_DIR").unwrap()),
        |value| value.path().to_path_buf(),
    );
    let path = directory.join("canonical.db");
    let mut source = Connection::open(&path).unwrap();
    source.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS committed_source(id INTEGER PRIMARY KEY,signed BLOB NOT NULL,events BLOB NOT NULL)").unwrap();
    let event = event();
    let key = SigningKey::from_bytes(&[7; 32]);
    let signed = signed(&event, &key);
    let count: i64 = source
        .query_row("SELECT count(*) FROM committed_source", [], |row| {
            row.get(0)
        })
        .unwrap();
    if std::env::var_os("PENELOPE_SAGA_RECOVER").is_some() {
        let cut = std::env::var("PENELOPE_SAGA_EXPECT_REPLAY").unwrap();
        let expected = i64::from(!matches!(
            cut.as_str(),
            "start-payload" | "source-precommit"
        ));
        assert_eq!(count, expected, "canonical source durability at {cut}");
    }
    if count == 0 {
        let transaction = source.transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO committed_source VALUES(1,?1,?2)",
                rusqlite::params![
                    serde_json::to_vec(&signed).unwrap(),
                    serde_json::to_vec(&vec![event.clone()]).unwrap()
                ],
            )
            .unwrap();
        sagas::cut("source-precommit");
        transaction.commit().unwrap();
    } else {
        assert_eq!(count, 1);
        let persisted: (Vec<u8>, Vec<u8>) = source
            .query_row(
                "SELECT signed,events FROM committed_source WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            persisted,
            (
                serde_json::to_vec(&signed).unwrap(),
                serde_json::to_vec(&vec![event.clone()]).unwrap()
            )
        );
    }
    sagas::cut("source-postcommit");
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
    sagas::cut("source-verified");
    // Engine event bytes remain the authoritative replay payload on outcomes.
    // The canonical input separately binds the verified upstream event bytes.
    batch.input.as_mut().unwrap().payload_digest = verified.event.payload_digest;
    run(store.put_payload(
        scope,
        verified.event.payload_digest,
        &canonicalize(&event).unwrap(),
    ))
    .unwrap();
    sagas::cut("source-payload");
    let batch = bind_verified_event_to_commit(batch, &verified).unwrap();
    sagas::cut("source-bound");
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

// Helpers run only under the kill harness and are selected by exact test name.
fn crash_helper(engine: &str, effectful: bool) {
    if std::env::var_os("PENELOPE_SAGA_CRASH_DIR").is_none() {
        return;
    }
    match engine {
        "linear" => sagas::run_linear_saga_with_source(effectful, &scope(), attach_source),
        "graph" => sagas::run_graph_saga_with_source(effectful, &scope(), attach_source),
        _ => unreachable!(),
    }
}
#[test]
fn canonical_pure_linear_crash_helper() {
    crash_helper("linear", false);
}
#[test]
fn canonical_pure_graph_crash_helper() {
    crash_helper("graph", false);
}
#[test]
fn canonical_effectful_linear_crash_helper() {
    crash_helper("linear", true);
}
#[test]
fn canonical_effectful_graph_crash_helper() {
    crash_helper("graph", true);
}
fn signed_crash_matrix(effectful: bool) {
    let mode = if effectful { "effectful" } else { "pure" };
    sagas::crash_matrix_with_source(
        effectful,
        |engine| format!("task_sagas::canonical_{mode}_{engine}_crash_helper"),
        &[
            "source-precommit",
            "source-postcommit",
            "source-verified",
            "source-payload",
            "source-bound",
        ],
    );
}
#[test]
fn signed_source_pure_sagas_recover_after_sigkill() {
    signed_crash_matrix(false);
}
#[test]
fn signed_source_effectful_sagas_recover_after_sigkill() {
    signed_crash_matrix(true);
}
