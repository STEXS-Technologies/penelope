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
    run_seeded_case(name, operations, conflict, None);
}
fn run_seeded_case(
    name: &str,
    operations: &[Operation; 2],
    conflict: bool,
    seed: Option<&AtomicProcessCommit>,
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
        if seed.is_some() {
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
    );
}
