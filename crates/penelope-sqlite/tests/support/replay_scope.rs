//! The checked page constructor rejects scope substitution anywhere in a page.
use super::*;

enum Component {
    Tenant,
    Process,
    Definition,
    Version,
    Digest,
}

#[test]
fn replay_rejects_each_scope_component_in_first_and_last_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scope.db");
    let adapter = store(&path, clock());
    let mut first = commit(false).outcomes.remove(0);
    first.tenant_id = format!("tnt_{}", "x".repeat(124)).parse().unwrap();
    first.process_id = format!("prc_{}", "x".repeat(124)).parse().unwrap();
    first.definition_id = format!("def_{}", "x".repeat(124)).parse().unwrap();
    first.definition_version = format!("dfv_{}", "x".repeat(124)).parse().unwrap();
    let mut last = first.clone();
    last.sequence = 1;
    last.outcome_id = id("out_second");
    last.kind = ProcessOutcomeKind::ActionPlanned;
    let expected = vec![first.clone(), last];
    let batch = AtomicProcessCommit::new(0, None, expected.clone(), Vec::new()).unwrap();
    block_on(ProcessStore::commit(&adapter, &batch)).unwrap();
    let request = OutcomeReplayRequest::new(first.scope(), 0, NonZeroU16::new(2).unwrap()).unwrap();
    let held = block_on(adapter.read_outcomes(&request)).unwrap();
    let connection = Connection::open(&path).unwrap();
    for original in &expected {
        for component in [
            Component::Tenant,
            Component::Process,
            Component::Definition,
            Component::Version,
            Component::Digest,
        ] {
            let mut substituted = original.clone();
            match component {
                Component::Tenant => substituted.tenant_id = id("tnt_other"),
                Component::Process => substituted.process_id = id("prc_other"),
                Component::Definition => substituted.definition_id = id("def_other"),
                Component::Version => substituted.definition_version = id("dfv_other"),
                Component::Digest => substituted.definition_digest = ContentDigest([17; 32]),
            }
            let bytes = serde_json::to_vec(&substituted).unwrap();
            connection
                .execute(
                    "UPDATE process_outcomes SET record=?1,checksum=?2 WHERE sequence=?3",
                    rusqlite::params![
                        bytes,
                        ContentDigest::sha256(&bytes).0.as_slice(),
                        i64::try_from(original.sequence).unwrap()
                    ],
                )
                .unwrap();
            let before =
                write_snapshot::snapshot(&connection, &["process_outcomes", "process_heads"]);
            assert_eq!(
                block_on(adapter.read_outcomes(&request)),
                Err(PortError::Invariant)
            );
            assert_eq!(
                write_snapshot::snapshot(&connection, &["process_outcomes", "process_heads"]),
                before
            );
            assert_eq!(held.outcomes, expected);
            let restored_bytes = serde_json::to_vec(original).unwrap();
            connection
                .execute(
                    "UPDATE process_outcomes SET record=?1,checksum=?2 WHERE sequence=?3",
                    rusqlite::params![
                        restored_bytes,
                        ContentDigest::sha256(&restored_bytes).0.as_slice(),
                        i64::try_from(original.sequence).unwrap()
                    ],
                )
                .unwrap();
            assert_eq!(
                block_on(adapter.read_outcomes(&request)).unwrap().outcomes,
                expected
            );
        }
    }
}
