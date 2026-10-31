//! Warm replay statements must bind every request and reread authoritative rows.
use super::*;
fn check(adapter: &SqliteProcessStore, expected: &[ProcessOutcome], from: u64, limit: u16) {
    let request =
        OutcomeReplayRequest::new(expected[0].scope(), from, NonZeroU16::new(limit).unwrap())
            .unwrap();
    let page = block_on(adapter.read_outcomes(&request)).unwrap();
    page.validate().unwrap();
    let selected: Vec<_> = expected
        .iter()
        .filter(|outcome| outcome.sequence >= from)
        .take(usize::from(limit))
        .cloned()
        .collect();
    let next = from + u64::try_from(selected.len()).unwrap();
    assert_eq!(page.scope, request.scope);
    assert_eq!(page.from_sequence, from);
    assert_eq!(page.outcomes, selected);
    assert_eq!(
        page.next_sequence,
        if next < u64::try_from(expected.len()).unwrap() {
            Some(next)
        } else {
            None
        }
    );
}
#[test]
fn warm_replay_rebinds_requests_recovers_from_corruption_and_sees_new_commits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replay-cache.db");
    let adapter = store(&path, clock());
    let mut first = commit(false);
    let mut second_outcome = first.outcomes[0].clone();
    second_outcome.sequence = 1;
    second_outcome.outcome_id = id("out_second");
    second_outcome.kind = ProcessOutcomeKind::ActionPlanned;
    first.outcomes.push(second_outcome);
    let mut other = commit(false);
    other.input.as_mut().unwrap().process_id = id("prc_other");
    other.outcomes[0].process_id = id("prc_other");
    other.actions[0].process_id = id("prc_other");
    block_on(ProcessStore::commit(&adapter, &first)).unwrap();
    block_on(ProcessStore::commit(&adapter, &other)).unwrap();
    for _ in 0..3 {
        check(&adapter, &first.outcomes, 1, 1);
        check(&adapter, &other.outcomes, 0, 2);
        check(&adapter, &first.outcomes, 0, 1);
        check(&adapter, &first.outcomes, 99, 2);
        check(&adapter, &first.outcomes, 0, 2);
    }
    let connection = Connection::open(&path).unwrap();
    let key = first.outcomes[0].scope().canonical_bytes();
    let original: Vec<u8> = connection
        .query_row(
            "SELECT checksum FROM process_outcomes WHERE scope=?1 AND sequence=0",
            [&key],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE process_outcomes SET checksum=zeroblob(32) WHERE scope=?1 AND sequence=0",
            [&key],
        )
        .unwrap();
    assert_eq!(
        block_on(adapter.read_outcomes(&replay())),
        Err(PortError::Invariant)
    );
    check(&adapter, &other.outcomes, 0, 1);
    check(&adapter, &first.outcomes, 1, 1);
    connection
        .execute(
            "UPDATE process_outcomes SET checksum=?2 WHERE scope=?1 AND sequence=0",
            rusqlite::params![key, original],
        )
        .unwrap();
    check(&adapter, &first.outcomes, 0, 2);
    connection
        .execute_batch("CREATE INDEX replay_test_index ON process_outcomes(scope,sequence)")
        .unwrap();
    check(&adapter, &first.outcomes, 0, 1);
    connection
        .execute_batch("DROP INDEX replay_test_index")
        .unwrap();
    check(&adapter, &other.outcomes, 0, 2);
    let mut appended = first.outcomes[1].clone();
    appended.sequence = 2;
    appended.outcome_id = id("out_third");
    let next = AtomicProcessCommit::new(2, None, vec![appended.clone()], Vec::new()).unwrap();
    block_on(ProcessStore::commit(&adapter, &next))
        .unwrap()
        .validate_for(&next)
        .unwrap();
    first.outcomes.push(appended);
    check(&adapter, &first.outcomes, 1, 1);
    check(&adapter, &first.outcomes, 2, 2);
    check(&adapter, &first.outcomes, 0, 3);
    check(&adapter, &other.outcomes, 0, 1);
}

#[test]
fn maximum_pages_refresh_across_workers_and_reject_corrupted_last_rows() {
    use super::write_snapshot::snapshot;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("replay-boundaries.db");
    let reader = store(&path, clock());
    let writer = store(&path, clock());
    let request = OutcomeReplayRequest::new(
        scope(),
        0,
        NonZeroU16::new(MAX_OUTCOMES_PER_READ_PAGE).unwrap(),
    )
    .unwrap();
    assert!(
        block_on(reader.read_outcomes(&request))
            .unwrap()
            .outcomes
            .is_empty()
    );
    let first = commit(false);
    let mut history = Vec::new();
    for sequence in 0..=u64::from(MAX_OUTCOMES_PER_READ_PAGE) {
        let mut outcome = first.outcomes[0].clone();
        outcome.sequence = sequence;
        outcome.outcome_id = id(&format!("out_boundary_{sequence}"));
        if sequence != 0 {
            outcome.kind = ProcessOutcomeKind::ActionPlanned;
        }
        history.push(outcome);
    }
    let boundary = usize::from(MAX_OUTCOMES_PER_READ_PAGE);
    for batch in history[..boundary].chunks(MAX_OUTCOMES_PER_ATOMIC_COMMIT) {
        let start = batch[0].sequence;
        let commit = AtomicProcessCommit::new(
            start,
            if start == 0 {
                first.input.clone()
            } else {
                None
            },
            batch.to_vec(),
            Vec::new(),
        )
        .unwrap();
        block_on(ProcessStore::commit(&writer, &commit)).unwrap();
    }
    let cursors = [0, 1, 127, 128, 511, 512, 529];
    let limits = [1, 127, 128, 511, MAX_OUTCOMES_PER_READ_PAGE];
    for from in cursors {
        for limit in limits {
            check(&reader, &history[..boundary], from, limit);
        }
    }
    let appended = AtomicProcessCommit::new(
        u64::from(MAX_OUTCOMES_PER_READ_PAGE),
        None,
        history[boundary..].to_vec(),
        Vec::new(),
    )
    .unwrap();
    block_on(ProcessStore::commit(&writer, &appended)).unwrap();
    for from in cursors {
        for limit in limits {
            check(&reader, &history, from, limit);
        }
    }
    let connection = Connection::open(&path).unwrap();
    let tables = [
        "process_usage",
        "process_inbox",
        "process_sources",
        "process_outcomes",
        "process_actions",
        "process_heads",
        "process_commits",
        "process_payloads",
        "process_timers",
        "lease_counter",
    ];
    let intact = snapshot(&connection, &tables);
    let last = i64::from(MAX_OUTCOMES_PER_READ_PAGE) - 1;
    let checksum: Vec<u8> = connection
        .query_row(
            "SELECT checksum FROM process_outcomes WHERE sequence=?1",
            [last],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE process_outcomes SET checksum=zeroblob(32) WHERE sequence=?1",
            [last],
        )
        .unwrap();
    let damaged = snapshot(&connection, &tables);
    for adapter in [&reader, &store(&path, clock())] {
        assert_eq!(
            block_on(adapter.read_outcomes(&request)),
            Err(PortError::Invariant)
        );
        assert_eq!(snapshot(&connection, &tables), damaged);
        // A healthy suffix outside the damaged page is still independently readable.
        check(adapter, &history, 512, MAX_OUTCOMES_PER_READ_PAGE);
    }
    connection
        .execute(
            "UPDATE process_outcomes SET checksum=?2 WHERE sequence=?1",
            rusqlite::params![last, checksum],
        )
        .unwrap();
    assert_eq!(snapshot(&connection, &tables), intact);
    check(&reader, &history, 0, MAX_OUTCOMES_PER_READ_PAGE);
    check(
        &store(&path, clock()),
        &history,
        1,
        MAX_OUTCOMES_PER_READ_PAGE,
    );
}
