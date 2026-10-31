//! Per-record encoding must clear old bytes and keep limits transactional.
use super::*;
use write_snapshot::snapshot;
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

fn batch(count: usize, varied: bool) -> AtomicProcessCommit {
    let mut batch = commit(false);
    let outcome = batch.outcomes[0].clone();
    let action = batch.actions[0].clone();
    batch.outcomes.clear();
    batch.actions.clear();
    for index in 0..count {
        let padding = if varied && index % 2 == 0 {
            "x".repeat(110)
        } else {
            String::new()
        };
        let mut row = outcome.clone();
        row.sequence = u64::try_from(index).unwrap();
        row.outcome_id = id(&format!("out_{padding}{index:04}"));
        row.payload_digest = ContentDigest([u8::try_from(index).unwrap(); 32]);
        batch.outcomes.push(row);
        let mut action_row = action.clone();
        action_row.action_id = id(&format!("act_{padding}{index:04}"));
        action_row.step_id = id(&format!("stp_{padding}{index:04}"));
        action_row.payload_digest = ContentDigest([u8::try_from(index).unwrap(); 32]);
        batch.actions.push(action_row);
    }
    batch.validate().unwrap();
    batch
}

fn check_rows(connection: &Connection, batch: &AtomicProcessCommit) {
    for row in &batch.outcomes {
        let (bytes, checksum): (Vec<u8>, Vec<u8>) = connection
            .query_row(
                "SELECT record,checksum FROM process_outcomes WHERE sequence=?1",
                [i64::try_from(row.sequence).unwrap()],
                |stored| Ok((stored.get(0)?, stored.get(1)?)),
            )
            .unwrap();
        assert_eq!(bytes, serde_json::to_vec(row).unwrap());
        assert_eq!(checksum, ContentDigest::sha256(&bytes).0);
    }
    for row in &batch.actions {
        let (bytes, checksum): (Vec<u8>, Vec<u8>) = connection
            .query_row(
                "SELECT action,checksum FROM process_actions WHERE action_id=?1",
                [row.action_id.as_str()],
                |stored| Ok((stored.get(0)?, stored.get(1)?)),
            )
            .unwrap();
        assert_eq!(bytes, serde_json::to_vec(row).unwrap());
        assert_eq!(checksum, ContentDigest::sha256(&bytes).0);
    }
}

#[test]
fn alternating_record_sizes_preserve_exact_bytes_checksums_and_reopened_retry() {
    for count in [1, 16, 128] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("encoding.db");
        let adapter = store(&path, clock());
        let batch = batch(count, true);
        let receipt = block_on(ProcessStore::commit(&adapter, &batch)).unwrap();
        receipt.validate_for(&batch).unwrap();
        assert!(!receipt.duplicate_input);
        let connection = Connection::open(&path).unwrap();
        check_rows(&connection, &batch);
        let before = snapshot(&connection, &TABLES);
        drop(adapter);
        let reopened = store(&path, clock());
        let duplicate = block_on(ProcessStore::commit(&reopened, &batch)).unwrap();
        duplicate.validate_for(&batch).unwrap();
        assert!(duplicate.duplicate_input);
        assert_eq!(snapshot(&connection, &TABLES), before);
        check_rows(&connection, &batch);
    }
    println!(
        "encoding_exact_bytes_profiles=3 maximum_rows=128 maximum_actions=128 reopened_retry=true"
    );
}

#[test]
fn late_record_insert_failure_rolls_back_then_retries_without_stale_encoding() {
    for component in ["outcome", "action"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("encoding-rollback.db");
        let good = batch(4, true);
        let adapter = store(&path, clock());
        let connection = Connection::open(&path).unwrap();
        let before = snapshot(&connection, &TABLES);
        let trigger = if component == "outcome" {
            "CREATE TRIGGER encoding_abort BEFORE INSERT ON process_outcomes WHEN NEW.sequence=3 BEGIN SELECT RAISE(ABORT,'late outcome'); END"
        } else {
            "CREATE TRIGGER encoding_abort BEFORE INSERT ON process_actions WHEN NEW.action_id='act_0003' BEGIN SELECT RAISE(ABORT,'late action'); END"
        };
        connection.execute_batch(trigger).unwrap();
        assert_eq!(
            block_on(ProcessStore::commit(&adapter, &good)),
            Err(PortError::Conflict),
            "{component}"
        );
        assert_eq!(snapshot(&connection, &TABLES), before);
        connection
            .execute_batch("DROP TRIGGER encoding_abort")
            .unwrap();
        block_on(ProcessStore::commit(&adapter, &good)).unwrap();
        check_rows(&connection, &good);
        let committed = snapshot(&connection, &TABLES);
        drop(adapter);
        let reopened = store(&path, clock());
        assert!(
            block_on(ProcessStore::commit(&reopened, &good))
                .unwrap()
                .duplicate_input
        );
        assert_eq!(snapshot(&connection, &TABLES), committed);
    }
    println!("encoding_late_insert_rollbacks=2 repaired=true reopened_retry=true");
}
