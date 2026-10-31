//! Outcome-only append must preserve the validated commit transaction contract.
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
fn outcome(scope: &ProcessScope, sequence: u64) -> ProcessOutcome {
    ProcessOutcome::new(
        scope.clone(),
        sequence,
        ProcessOutcomeFact::new(
            id(&format!("out_append_{sequence:04}")),
            CausationId::Input(id("inp_append")),
            OutcomeActor::System,
            LogicalTime(sequence),
            ProcessOutcomeKind::ActionPlanned,
            ContentDigest([2; 32]),
        ),
    )
}
fn fixture_scope(length: usize) -> ProcessScope {
    let suffix = "x".repeat(length - 4);
    ProcessScope::new(
        id(&format!("tnt_{suffix}")),
        id(&format!("prc_{suffix}")),
        id(&format!("def_{suffix}")),
        id(&format!("dfv_{suffix}")),
        ContentDigest([9; 32]),
    )
}
#[test]
fn append_checks_every_scope_field_before_copying_or_writing_any_rows() {
    for length in [5, 22, 23, 128] {
        let pinned = fixture_scope(length);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("scope.db");
        let adapter = store(&path, clock());
        let connection = Connection::open(&path).unwrap();
        block_on(adapter.append_outcomes(&pinned, 0, &[outcome(&pinned, 0)])).unwrap();
        let before = write_snapshot::snapshot(&connection, &TABLES);
        let good = [outcome(&pinned, 1), outcome(&pinned, 2)];
        for rank in [0, 1] {
            for component in 0..5 {
                let mut wrong = good.clone();
                match component {
                    0 => wrong[rank].tenant_id = id("tnt_wrong"),
                    1 => wrong[rank].process_id = id("prc_wrong"),
                    2 => wrong[rank].definition_id = id("def_wrong"),
                    3 => wrong[rank].definition_version = id("dfv_wrong"),
                    4 => wrong[rank].definition_digest = ContentDigest([8; 32]),
                    _ => panic!("invalid scope fixture component"),
                }
                assert_eq!(
                    block_on(adapter.append_outcomes(&pinned, 1, &wrong)),
                    Err(PortError::Invariant)
                );
                assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            }
        }
        block_on(adapter.append_outcomes(&pinned, 1, &good)).unwrap();
        let committed = write_snapshot::snapshot(&connection, &TABLES);
        drop(adapter);
        let reopened = store(&path, clock());
        block_on(reopened.append_outcomes(&pinned, 1, &good)).unwrap();
        assert_eq!(write_snapshot::snapshot(&connection, &TABLES), committed);
        let request = OutcomeReplayRequest::new(pinned, 0, NonZeroU16::new(10).unwrap()).unwrap();
        let page = block_on(reopened.read_outcomes(&request)).unwrap();
        assert_eq!(page.outcomes.len(), 3);
        assert_eq!(&page.outcomes[1..], &good);
        assert_eq!(page.next_sequence, None);
    }
}
#[test]
fn append_matches_commit_rollback_retry_and_bounds_at_empty_and_seeded_frontiers() {
    for seeded in [false, true] {
        for count in [1, MAX_OUTCOMES_PER_ATOMIC_COMMIT] {
            let directory = tempfile::tempdir().unwrap();
            let pinned = fixture_scope(128);
            let start = u64::from(seeded);
            let outcomes: Vec<_> = (0..count)
                .map(|offset| outcome(&pinned, start + u64::try_from(offset).unwrap()))
                .collect();
            let reference = store(&directory.path().join("reference.db"), clock());
            let path = directory.path().join("append.db");
            let adapter = store(&path, clock());
            for target in [&reference, &adapter] {
                if seeded {
                    block_on(target.append_outcomes(&pinned, 0, &[outcome(&pinned, 0)])).unwrap();
                }
            }
            let reference_connection =
                Connection::open(directory.path().join("reference.db")).unwrap();
            let connection = Connection::open(&path).unwrap();
            let before = write_snapshot::snapshot(&connection, &TABLES);
            assert_eq!(
                block_on(adapter.append_outcomes(&pinned, start, &[])),
                Err(PortError::Invariant)
            );
            assert_eq!(
                block_on(adapter.append_outcomes(
                    &pinned,
                    start,
                    &vec![outcome(&pinned, start); MAX_OUTCOMES_PER_ATOMIC_COMMIT + 1]
                )),
                Err(PortError::QuotaExceeded)
            );
            let mut invalid = outcomes.clone();
            invalid[0].sequence = start + 1;
            assert_eq!(
                block_on(adapter.append_outcomes(&pinned, start, &invalid)),
                Err(PortError::Invariant)
            );
            if count > 1 {
                invalid = outcomes.clone();
                invalid[1].outcome_id = invalid[0].outcome_id.clone();
                assert_eq!(
                    block_on(adapter.append_outcomes(&pinned, start, &invalid)),
                    Err(PortError::Invariant)
                );
            }
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            let last = outcomes.last().unwrap().sequence;
            connection.execute_batch(&format!("CREATE TRIGGER fail_append BEFORE INSERT ON process_outcomes WHEN NEW.sequence={last} BEGIN SELECT RAISE(ABORT,'injected append failure'); END;")).unwrap();
            assert_eq!(
                block_on(adapter.append_outcomes(&pinned, start, &outcomes)),
                Err(PortError::Conflict)
            );
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            connection
                .execute_batch("DROP TRIGGER fail_append")
                .unwrap();
            let committed =
                AtomicProcessCommit::new(start, None, outcomes.clone(), Vec::new()).unwrap();
            let receipt = block_on(ProcessStore::commit(&reference, &committed)).unwrap();
            receipt.validate_for(&committed).unwrap();
            block_on(adapter.append_outcomes(&pinned, start, &outcomes)).unwrap();
            let expected = write_snapshot::snapshot(&reference_connection, &TABLES);
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), expected);
            drop(adapter);
            let reopened = store(&path, clock());
            block_on(reopened.append_outcomes(&pinned, start, &outcomes)).unwrap();
            let mut conflicting = outcomes.clone();
            conflicting[0].payload_digest = ContentDigest([99; 32]);
            assert_eq!(
                block_on(reopened.append_outcomes(&pinned, start, &conflicting)),
                Err(PortError::Conflict)
            );
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), expected);
            let request =
                OutcomeReplayRequest::new(pinned.clone(), 0, NonZeroU16::new(128).unwrap())
                    .unwrap();
            let page = block_on(reopened.read_outcomes(&request)).unwrap();
            let mut history = if seeded {
                vec![outcome(&pinned, 0)]
            } else {
                Vec::new()
            };
            history.extend(outcomes);
            assert_eq!(page.outcomes, &history[..history.len().min(128)]);
            assert_eq!(
                page.next_sequence,
                if history.len() > 128 { Some(128) } else { None }
            );
        }
    }
}

struct Notice(std::sync::mpsc::Sender<()>);
impl Wake for Notice {
    fn wake(self: Arc<Self>) {
        let _notification = self.0.send(());
    }
}
#[test]
fn admitted_append_survives_caller_cancellation_and_reopened_exact_retry() {
    for count in [1, MAX_OUTCOMES_PER_ATOMIC_COMMIT] {
        for after_completion in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("cancel.db");
            let pinned = fixture_scope(128);
            let outcomes: Vec<_> = (0..count)
                .map(|sequence| outcome(&pinned, u64::try_from(sequence).unwrap()))
                .collect();
            let adapter = store(&path, clock());
            let connection = Connection::open(&path).unwrap();
            let before = write_snapshot::snapshot(&connection, &TABLES);
            connection.execute_batch("BEGIN IMMEDIATE").unwrap();
            let (notice, completed) = std::sync::mpsc::channel();
            let waker = Waker::from(Arc::new(Notice(notice)));
            let mut context = Context::from_waker(&waker);
            let mut canceled = adapter.append_outcomes(&pinned, 0, &outcomes);
            assert!(canceled.as_mut().poll(&mut context).is_pending());
            let request =
                OutcomeReplayRequest::new(pinned.clone(), 0, NonZeroU16::new(128).unwrap())
                    .unwrap();
            let mut follower = adapter.read_outcomes(&request);
            assert!(follower.as_mut().poll(&mut context).is_pending());
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            if after_completion {
                connection.execute_batch("COMMIT").unwrap();
                completed.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            drop(canceled);
            if !after_completion {
                connection.execute_batch("COMMIT").unwrap();
            }
            // The live follower is behind the admitted append on the worker.
            // Do not require a canceled response to keep or wake a registration.
            let page = block_on(follower).unwrap();
            assert_eq!(page.outcomes, outcomes);
            assert_eq!(page.next_sequence, None);
            let expected = write_snapshot::snapshot(&connection, &TABLES);
            drop(adapter);
            let reopened = store(&path, clock());
            block_on(reopened.append_outcomes(&pinned, 0, &outcomes)).unwrap();
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), expected);
            let committed =
                AtomicProcessCommit::new(0, None, outcomes.clone(), Vec::new()).unwrap();
            let reference_path = directory.path().join("reference.db");
            let reference = store(&reference_path, clock());
            let receipt = block_on(ProcessStore::commit(&reference, &committed)).unwrap();
            receipt.validate_for(&committed).unwrap();
            let reference_connection = Connection::open(&reference_path).unwrap();
            assert_eq!(
                write_snapshot::snapshot(&reference_connection, &TABLES),
                expected
            );
            println!(
                "append_cancellation count={count} after_completion={after_completion} exact_state=true reopened_retry=true"
            );
        }
    }
}
