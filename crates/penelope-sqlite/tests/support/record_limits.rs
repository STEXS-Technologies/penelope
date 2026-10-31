//! A larger record allowance must not reject an otherwise identical small commit.
use super::*;
#[test]
fn raising_record_limit_preserves_commit_retry_replay_and_reopen() {
    let factor =
        u32::try_from(MAX_OUTCOMES_PER_ATOMIC_COMMIT + MAX_ACTIONS_PER_ATOMIC_COMMIT + 2).unwrap();
    let boundary = u32::MAX / factor;
    let mut failures = Vec::new();
    for limit in [
        4096,
        SqliteLimits::default().max_record_bytes,
        boundary,
        boundary + 1,
        u32::MAX,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("record-limit.db");
        let limits = SqliteLimits {
            max_record_bytes: limit,
            ..SqliteLimits::default()
        };
        let adapter = SqliteProcessStore::open(&path, limits, clock(), 10, 8).unwrap();
        let connection = Connection::open(&path).unwrap();
        let tables = [
            "process_usage",
            "process_inbox",
            "process_outcomes",
            "process_actions",
            "process_heads",
            "process_commits",
            "lease_counter",
        ];
        let before = write_snapshot::snapshot(&connection, &tables);
        let batch = commit(false);
        let result = block_on(ProcessStore::commit(&adapter, &batch));
        println!("record_limit={limit} first_commit={result:?}");
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(error) => {
                assert_eq!(write_snapshot::snapshot(&connection, &tables), before);
                failures.push((limit, error));
                continue;
            }
        };
        receipt.validate_for(&batch).unwrap();
        assert!(!receipt.duplicate_input);
        let duplicate = block_on(ProcessStore::commit(&adapter, &batch)).unwrap();
        duplicate.validate_for(&batch).unwrap();
        assert!(duplicate.duplicate_input);
        assert_eq!(
            block_on(adapter.read_outcomes(&replay())).unwrap().outcomes,
            batch.outcomes
        );
        let leases = block_on(adapter.claim(&claim())).unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].record.action, batch.actions[0]);
        block_on(OutboxStore::acknowledge(
            &adapter,
            &leases[0],
            LogicalTime(100),
        ))
        .unwrap();
        drop(adapter);
        let reopened = SqliteProcessStore::open(&path, limits, clock(), 10, 8).unwrap();
        let recovered = block_on(ProcessStore::commit(&reopened, &batch)).unwrap();
        recovered.validate_for(&batch).unwrap();
        assert!(recovered.duplicate_input);
        assert_eq!(
            block_on(reopened.read_outcomes(&replay()))
                .unwrap()
                .outcomes,
            batch.outcomes
        );
        assert!(block_on(reopened.claim(&claim())).unwrap().is_empty());
        let next: u32 = connection
            .query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))
            .unwrap();
        assert_eq!(next, 2);
    }
    assert!(
        failures.is_empty(),
        "raising record limits rejected small commits: {failures:?}"
    );
}
