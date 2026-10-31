//! Independent admission/commit workers must produce a complete legal serial history.
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
#[derive(Clone)]
enum Operation {
    Accept(Box<ProcessInput>),
    Commit(Box<AtomicProcessCommit>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Receipt {
    Input(bool),
    Commit(bool),
}
impl Operation {
    async fn apply(&self, adapter: &SqliteProcessStore) -> Result<Receipt, PortError> {
        match self {
            Self::Accept(input) => {
                let receipt = adapter.accept(input).await?;
                assert_eq!(receipt.tenant_id, input.tenant_id);
                assert_eq!(receipt.process_id, input.process_id);
                assert_eq!(receipt.input_id, input.input_id);
                Ok(Receipt::Input(receipt.duplicate))
            }
            Self::Commit(committed) => {
                let receipt = ProcessStore::commit(adapter, committed).await?;
                receipt.validate_for(committed).unwrap();
                Ok(Receipt::Commit(receipt.duplicate_input))
            }
        }
    }
}
fn other_definition(original: &AtomicProcessCommit) -> AtomicProcessCommit {
    let mut changed = original.clone();
    assert_ne!(
        original.outcomes[0].definition_digest,
        ContentDigest([10; 32])
    );
    for outcome in &mut changed.outcomes {
        outcome.definition_digest = ContentDigest([10; 32]);
    }
    for action in &mut changed.actions {
        action.definition_digest = ContentDigest([10; 32]);
    }
    changed.validate().unwrap();
    changed
}
#[derive(Clone, Copy)]
enum Model {
    Standard,
    SeededRejection,
    CompetingBatches,
    RecoveringBatches,
}
struct Serial {
    state: write_snapshot::Snapshot,
    results: [Result<Receipt, PortError>; 2],
}
fn serial(
    path: &std::path::Path,
    operations: &[Operation; 2],
    order: usize,
    seed: Option<&AtomicProcessCommit>,
) -> Serial {
    let adapter = store(path, clock());
    if let Some(seed) = seed {
        block_on(ProcessStore::commit(&adapter, seed)).unwrap();
    }
    let connection = Connection::open(path).unwrap();
    let mut results = [Err(PortError::Invariant); 2];
    for index in [order, 1 - order] {
        results[index] = block_on(operations[index].apply(&adapter));
    }
    Serial {
        state: write_snapshot::snapshot(&connection, &TABLES),
        results,
    }
}
fn run_case(name: &str, operations: &[Operation; 2], conflict: bool) {
    run_seeded_case(name, operations, conflict, None, Model::Standard);
}
fn run_seeded_case(
    name: &str,
    operations: &[Operation; 2],
    conflict: bool,
    seed: Option<&AtomicProcessCommit>,
    model: Model,
) {
    let directory = tempfile::tempdir().unwrap();
    let references = [
        serial(&directory.path().join("left-first.db"), operations, 0, seed),
        serial(
            &directory.path().join("right-first.db"),
            operations,
            1,
            seed,
        ),
    ];
    let path = directory.path().join("raced.db");
    let first = store(&path, clock());
    if let Some(seed) = seed {
        block_on(ProcessStore::commit(&first, seed)).unwrap();
    }
    let second = store(&path, clock());
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (admitted, ready) = std::sync::mpsc::channel();
    let handles = [
        (first, operations[0].clone()),
        (second, operations[1].clone()),
    ]
    .map(|(adapter, operation)| {
        let admitted = admitted.clone();
        std::thread::spawn(move || {
            let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
            let mut context = Context::from_waker(&waker);
            let mut future = Box::pin(operation.apply(&adapter));
            assert!(future.as_mut().poll(&mut context).is_pending());
            admitted.send(()).unwrap();
            block_on(future)
        })
    });
    for _ in 0..2 {
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    connection.execute_batch("COMMIT").unwrap();
    let results = handles.map(|handle| handle.join().unwrap());
    let state = write_snapshot::snapshot(&connection, &TABLES);
    let accepted = results.iter().filter(|result| result.is_ok()).count();
    println!("case={name} admitted=2 accepted={accepted}");
    assert_eq!(
        accepted,
        if conflict { 1 } else { 2 },
        "{name}: {results:?}"
    );
    for (order, reference) in references.iter().enumerate() {
        let serial_results = &reference.results;
        if matches!(model, Model::SeededRejection) {
            assert_eq!(serial_results[0], Err(PortError::Conflict));
            assert_eq!(serial_results[1], Ok(Receipt::Input(true)));
            continue;
        }
        assert!(matches!(
            serial_results[order],
            Ok(Receipt::Input(false) | Receipt::Commit(false))
        ));
        if conflict {
            assert_eq!(serial_results[1 - order], Err(PortError::Conflict));
        } else {
            assert!(matches!(
                serial_results[1 - order],
                Ok(Receipt::Input(true) | Receipt::Commit(true))
            ));
        }
    }
    let order = references
        .iter()
        .position(|reference| reference.results == results && reference.state == state)
        .unwrap_or_else(|| panic!("{name}: no legal serial history for {results:?}"));
    if matches!(model, Model::CompetingBatches | Model::RecoveringBatches) {
        verify_winner(&connection, operations, results, seed);
    }
    let reopened = store(&path, clock());
    for (index, operation) in operations.iter().enumerate() {
        let retry = block_on(operation.apply(&reopened));
        let expected = match results[index] {
            Ok(Receipt::Input(_)) => Ok(Receipt::Input(true)),
            Ok(Receipt::Commit(_)) => Ok(Receipt::Commit(true)),
            Err(error) => Err(error),
        };
        assert_eq!(retry, expected, "{name} reopened index={index}");
        assert_eq!(write_snapshot::snapshot(&connection, &TABLES), state);
    }
    drop(reopened);
    if matches!(model, Model::RecoveringBatches) {
        recover_loser(&path, &connection, operations, results, seed);
        println!("case={name} recovered=true exact_history=true consumed_again=false");
    }
    println!(
        "case={name} admitted=2 accepted={accepted} serial_model={order} exact_state=true reopened_retry=true"
    );
}
#[test]
fn admission_and_commit_races_preserve_identity_consumption_and_source_binding() {
    let first = commit(false);
    let input = first.input.as_ref().unwrap().clone();
    run_case(
        "duplicate-admission",
        &[
            Operation::Accept(Box::new(input.clone())),
            Operation::Accept(Box::new(input.clone())),
        ],
        false,
    );
    run_case(
        "admission-commit",
        &[
            Operation::Accept(Box::new(input.clone())),
            Operation::Commit(Box::new(first.clone())),
        ],
        false,
    );
    run_case(
        "duplicate-commit",
        &[
            Operation::Commit(Box::new(first.clone())),
            Operation::Commit(Box::new(first.clone())),
        ],
        false,
    );
    let mut conflicting = input;
    conflicting.payload_digest = ContentDigest([8; 32]);
    run_case(
        "conflicting-admission",
        &[
            Operation::Accept(Box::new(first.input.as_ref().unwrap().clone())),
            Operation::Accept(Box::new(conflicting)),
        ],
        true,
    );
    run_case(
        "shared-input-across-definitions",
        &[
            Operation::Commit(Box::new(first.clone())),
            Operation::Commit(Box::new(other_definition(&first))),
        ],
        true,
    );
    let sourced = first.with_canonical_source_event(id("cev_shared"));
    let mut competing = other_definition(&sourced);
    competing.input.as_mut().unwrap().input_id = id("inp_competing");
    competing.outcomes[0].causation_id = CausationId::Input(id("inp_competing"));
    run_case(
        "shared-source-across-definitions",
        &[
            Operation::Commit(Box::new(sourced)),
            Operation::Commit(Box::new(competing)),
        ],
        true,
    );
}

fn next_commit(original: &AtomicProcessCommit, input_id: InputId) -> AtomicProcessCommit {
    let mut next = original.clone();
    next.expected_sequence = 1;
    next.input.as_mut().unwrap().input_id = input_id.clone();
    next.outcomes[0].sequence = 1;
    next.outcomes[0].outcome_id = id("out_next");
    next.outcomes[0].causation_id = CausationId::Input(input_id);
    next.actions[0].action_id = id("act_next");
    next.actions[0].step_id = id("stp_next");
    next.validate().unwrap();
    next
}
#[test]
fn next_frontier_rejects_consumed_inputs_and_reused_sources_during_admission() {
    let consumed = commit(false);
    let input = consumed.input.as_ref().unwrap().clone();
    run_seeded_case(
        "consumed-input-at-next-frontier",
        &[
            Operation::Commit(Box::new(next_commit(&consumed, input.input_id.clone()))),
            Operation::Accept(Box::new(input)),
        ],
        true,
        Some(&consumed),
        Model::SeededRejection,
    );
    let sourced = commit(false).with_canonical_source_event(id("cev_seed"));
    let sourced_input = sourced.input.as_ref().unwrap().clone();
    run_seeded_case(
        "reused-source-at-next-frontier",
        &[
            Operation::Commit(Box::new(next_commit(&sourced, id("inp_competing")))),
            Operation::Accept(Box::new(sourced_input)),
        ],
        true,
        Some(&sourced),
        Model::SeededRejection,
    );
}

fn competing_batch(tag: &str, first_sequence: u64, byte: u8) -> AtomicProcessCommit {
    let template = commit(false);
    let mut input = template.input.unwrap();
    input.input_id = id(&format!("inp_{tag}"));
    input.payload_digest = ContentDigest([byte; 32]);
    let mut outcomes = Vec::new();
    let mut actions = Vec::new();
    for offset in 0..2 {
        let mut outcome = template.outcomes[0].clone();
        outcome.sequence = first_sequence + offset;
        outcome.outcome_id = id(&format!("out_{tag}_{offset}"));
        outcome.causation_id = CausationId::Input(input.input_id.clone());
        outcome.payload_digest = input.payload_digest;
        if outcome.sequence != 0 {
            outcome.kind = ProcessOutcomeKind::ActionPlanned;
        }
        outcomes.push(outcome);
        let mut action = template.actions[0].clone();
        action.action_id = id(&format!("act_{tag}_{offset}"));
        action.step_id = id(&format!("stp_{tag}_{offset}"));
        action.payload_digest = input.payload_digest;
        actions.push(action);
    }
    AtomicProcessCommit::new(first_sequence, Some(input), outcomes, actions)
        .unwrap()
        .with_canonical_source_event(id(&format!("cev_{tag}")))
}

fn records<T: serde::de::DeserializeOwned>(connection: &Connection, sql: &str) -> Vec<T> {
    connection
        .prepare(sql)
        .unwrap()
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|row| serde_json::from_slice(&row.unwrap()).unwrap())
        .collect()
}

fn verify_winner(
    connection: &Connection,
    operations: &[Operation; 2],
    results: [Result<Receipt, PortError>; 2],
    seed: Option<&AtomicProcessCommit>,
) {
    let winner = results.iter().position(Result::is_ok).unwrap();
    let Operation::Commit(committed) = &operations[winner] else {
        panic!("expected commit");
    };
    let batches: Vec<_> = seed
        .into_iter()
        .chain(std::iter::once(committed.as_ref()))
        .collect();
    verify_batches(connection, &batches);
}

fn verify_batches(connection: &Connection, batches: &[&AtomicProcessCommit]) {
    let expected_outcomes: Vec<_> = batches
        .iter()
        .flat_map(|batch| batch.outcomes.clone())
        .collect();
    let mut expected_actions: Vec<_> = batches
        .iter()
        .flat_map(|batch| batch.actions.clone())
        .collect();
    expected_actions.sort_by(|left, right| left.action_id.cmp(&right.action_id));
    let mut expected_inputs: Vec<_> = batches
        .iter()
        .filter_map(|batch| batch.input.clone())
        .collect();
    expected_inputs.sort_by(|left, right| left.input_id.cmp(&right.input_id));
    assert_eq!(
        records::<ProcessOutcome>(
            connection,
            "SELECT record FROM process_outcomes ORDER BY sequence"
        ),
        expected_outcomes
    );
    assert_eq!(
        records::<ProcessAction>(
            connection,
            "SELECT action FROM process_actions ORDER BY action_id"
        ),
        expected_actions
    );
    assert_eq!(
        records::<ProcessInput>(
            connection,
            "SELECT input FROM process_inbox ORDER BY input_id"
        ),
        expected_inputs
    );
    let mut expected_sources: Vec<_> = batches
        .iter()
        .filter_map(|batch| {
            batch.canonical_source_event_id.as_ref().map(|source| {
                (
                    source.as_str().to_owned(),
                    batch.input.as_ref().unwrap().input_id.as_str().to_owned(),
                )
            })
        })
        .collect();
    expected_sources.sort();
    let sources: Vec<(String, String)> = connection
        .prepare("SELECT event_id,input_id FROM process_sources ORDER BY event_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(sources, expected_sources);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM process_commits", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        i64::try_from(batches.len()).unwrap()
    );
    assert_eq!(
        connection
            .query_row("SELECT next_sequence FROM process_heads", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        i64::try_from(expected_outcomes.len()).unwrap()
    );
}

#[test]
fn different_batches_at_one_frontier_commit_exactly_one_complete_winner() {
    for seeded in [false, true] {
        let seed = commit(false);
        let first_sequence = u64::from(seeded);
        let operations = [
            Operation::Commit(Box::new(competing_batch("left", first_sequence, 31))),
            Operation::Commit(Box::new(competing_batch("right", first_sequence, 47))),
        ];
        for _ in 0..8 {
            run_seeded_case(
                if seeded {
                    "competing-next-frontier"
                } else {
                    "competing-empty-frontier"
                },
                &operations,
                true,
                seeded.then_some(&seed),
                Model::CompetingBatches,
            );
        }
    }
}

fn at_frontier(original: &AtomicProcessCommit, frontier: u64) -> AtomicProcessCommit {
    let mut batch = original.clone();
    batch.expected_sequence = frontier;
    for (offset, outcome) in batch.outcomes.iter_mut().enumerate() {
        outcome.sequence = frontier + u64::try_from(offset).unwrap();
        outcome.kind = ProcessOutcomeKind::ActionPlanned;
    }
    batch.validate().unwrap();
    batch
}

fn recover_loser(
    path: &std::path::Path,
    connection: &Connection,
    operations: &[Operation; 2],
    results: [Result<Receipt, PortError>; 2],
    seed: Option<&AtomicProcessCommit>,
) {
    let winner_index = results.iter().position(Result::is_ok).unwrap();
    let Operation::Commit(winner) = &operations[winner_index] else {
        panic!("expected commit");
    };
    let Operation::Commit(loser) = &operations[1 - winner_index] else {
        panic!("expected commit");
    };
    let recovering = store(path, clock());
    let request =
        OutcomeReplayRequest::new(winner.outcomes[0].scope(), 0, NonZeroU16::new(10).unwrap())
            .unwrap();
    let page = block_on(recovering.read_outcomes(&request)).unwrap();
    assert_eq!(page.next_sequence, None);
    let frontier = page.outcomes.last().unwrap().sequence + 1;
    let recovered = at_frontier(loser, frontier);
    let receipt = block_on(ProcessStore::commit(&recovering, &recovered)).unwrap();
    receipt.validate_for(&recovered).unwrap();
    assert!(!receipt.duplicate_input);
    let batches: Vec<_> = seed
        .into_iter()
        .chain([winner.as_ref(), &recovered])
        .collect();
    verify_batches(connection, &batches);
    let retained = write_snapshot::snapshot(connection, &TABLES);
    drop(recovering);
    let reopened = store(path, clock());
    for batch in &batches {
        let duplicate = block_on(ProcessStore::commit(&reopened, batch)).unwrap();
        duplicate.validate_for(batch).unwrap();
        assert!(duplicate.duplicate_input);
        assert_eq!(write_snapshot::snapshot(connection, &TABLES), retained);
    }
    assert_eq!(
        block_on(ProcessStore::commit(&reopened, loser)),
        Err(PortError::Conflict)
    );
    let next = receipt.committed_through_sequence + 1;
    let mut consumed_again = at_frontier(&recovered, next);
    // Remove the source-event collision and choose fresh record identities so
    // only consumed-input fencing can reject this otherwise valid new batch.
    consumed_again.canonical_source_event_id = None;
    for (offset, outcome) in consumed_again.outcomes.iter_mut().enumerate() {
        outcome.outcome_id = id(&format!("out_again_{offset}"));
    }
    for (offset, action) in consumed_again.actions.iter_mut().enumerate() {
        action.action_id = id(&format!("act_again_{offset}"));
        action.step_id = id(&format!("stp_again_{offset}"));
    }
    consumed_again.validate().unwrap();
    assert_eq!(
        block_on(ProcessStore::commit(&reopened, &consumed_again)),
        Err(PortError::Conflict)
    );
    assert_eq!(write_snapshot::snapshot(connection, &TABLES), retained);
    let restored = block_on(reopened.read_outcomes(&request)).unwrap();
    assert_eq!(
        restored.outcomes,
        batches
            .iter()
            .flat_map(|batch| batch.outcomes.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(restored.next_sequence, None);
}

#[test]
fn losing_batch_recovers_at_the_reloaded_frontier_and_consumes_its_input_once() {
    for seeded in [false, true] {
        let seed = commit(false);
        let operations = [
            Operation::Commit(Box::new(competing_batch("left", u64::from(seeded), 31))),
            Operation::Commit(Box::new(competing_batch("right", u64::from(seeded), 47))),
        ];
        for _ in 0..8 {
            run_seeded_case(
                if seeded {
                    "recover-next-frontier"
                } else {
                    "recover-empty-frontier"
                },
                &operations,
                true,
                seeded.then_some(&seed),
                Model::RecoveringBatches,
            );
        }
    }
}
