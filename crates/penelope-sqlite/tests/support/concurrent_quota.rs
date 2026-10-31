//! Independent workers must linearize shared byte and process-admission quotas.
use super::*;
use penelope_ports::payloads::ProcessPayloadStore;

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
enum Write {
    Payload(Box<ProcessScope>, [u8; 16]),
    Inbox(Box<ProcessInput>),
    Commit(Box<AtomicProcessCommit>),
    Timer(Box<TimerSchedule>),
}
impl Write {
    async fn apply(&self, adapter: &SqliteProcessStore) -> Result<(), PortError> {
        match self {
            Self::Payload(scope, bytes) => {
                let digest = ContentDigest::sha256(bytes);
                if adapter.put_payload(scope, digest, bytes).await? != digest {
                    return Err(PortError::Invariant);
                }
                Ok(())
            }
            Self::Inbox(input) => adapter.accept(input).await.map(|_receipt| ()),
            Self::Commit(committed) => {
                let receipt = ProcessStore::commit(adapter, committed).await?;
                receipt
                    .validate_for(committed)
                    .map_err(|_invalid| PortError::Invariant)
            }
            Self::Timer(timer) => adapter.schedule(timer).await,
        }
    }
}
fn scoped_commit(scope: &ProcessScope, timer: bool) -> AtomicProcessCommit {
    let mut committed = commit(timer);
    let input = committed.input.as_mut().unwrap();
    input.tenant_id = scope.tenant_id.clone();
    input.process_id = scope.process_id.clone();
    for outcome in &mut committed.outcomes {
        outcome.tenant_id = scope.tenant_id.clone();
        outcome.process_id = scope.process_id.clone();
        outcome.definition_id = scope.definition_id.clone();
        outcome.definition_version = scope.definition_version.clone();
        outcome.definition_digest = scope.definition_digest;
    }
    for action in &mut committed.actions {
        action.tenant_id = scope.tenant_id.clone();
        action.process_id = scope.process_id.clone();
        action.definition_id = scope.definition_id.clone();
        action.definition_version = scope.definition_version.clone();
        action.definition_digest = scope.definition_digest;
    }
    committed
}
fn input(scope: &ProcessScope, identity: &str) -> Write {
    Write::Inbox(Box::new(ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id(identity),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([5; 32]),
    )))
}
fn usage(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT coalesce(sum(bytes_used),0) FROM process_usage",
            [],
            |row| row.get(0),
        )
        .unwrap()
}
fn prepare(
    path: &std::path::Path,
    limits: SqliteLimits,
    seed: Option<&Write>,
) -> (SqliteProcessStore, Connection) {
    let adapter = SqliteProcessStore::open(path, limits, clock(), 10, 16).unwrap();
    if let Some(seed) = seed {
        block_on(seed.apply(&adapter)).unwrap();
    }
    (adapter, Connection::open(path).unwrap())
}
struct Reference {
    state: write_snapshot::Snapshot,
    initial_usage: i64,
    added: i64,
}
fn reference(path: &std::path::Path, seed: Option<&Write>, operation: &Write) -> Reference {
    let (adapter, connection) = prepare(path, SqliteLimits::default(), seed);
    let initial_usage = usage(&connection);
    block_on(operation.apply(&adapter)).unwrap();
    let added = usage(&connection).checked_sub(initial_usage).unwrap();
    assert!(added > 0);
    Reference {
        state: write_snapshot::snapshot(&connection, &TABLES),
        initial_usage,
        added,
    }
}
fn run_case(kind: &str, name: &str, left: Write, right: Write, seed: Option<&Write>) {
    let directory = tempfile::tempdir().unwrap();
    let reference_paths = [
        directory.path().join("left.db"),
        directory.path().join("right.db"),
    ];
    let operations = [left, right];
    let references = [
        reference(&reference_paths[0], seed, &operations[0]),
        reference(&reference_paths[1], seed, &operations[1]),
    ];
    assert_eq!(references[0].initial_usage, references[1].initial_usage);
    let limits = if kind == "bytes" {
        SqliteLimits {
            max_scope_bytes: u64::try_from(
                references[0]
                    .initial_usage
                    .checked_add(references[0].added.max(references[1].added))
                    .unwrap(),
            )
            .unwrap(),
            ..SqliteLimits::default()
        }
    } else {
        assert_eq!(references[0].initial_usage, 0);
        SqliteLimits {
            max_scopes: 1,
            ..SqliteLimits::default()
        }
    };
    let path = directory.path().join("raced.db");
    let (first, connection) = prepare(&path, limits, seed);
    let second = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
    // Each public future must enqueue and become Pending while this separate
    // connection owns the writer lock. No sleeps or assumed thread overlap.
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
    let accepted = results.iter().filter(|result| result.is_ok()).count();
    println!("kind={kind} case={name} admitted=2 accepted={accepted}");
    assert_eq!(accepted, 1, "{kind}/{name}: {results:?}");
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(PortError::QuotaExceeded))
            .count(),
        1
    );
    let winner = results.iter().position(Result::is_ok).unwrap();
    assert_eq!(
        write_snapshot::snapshot(&connection, &TABLES),
        references[winner].state
    );
    // Both serial orders are independently valid models: either operation fits
    // alone, and its competitor must reject without leaving partial writes.
    for index in 0..2 {
        let adapter =
            SqliteProcessStore::open(&reference_paths[index], limits, clock(), 10, 16).unwrap();
        let inspection = Connection::open(&reference_paths[index]).unwrap();
        assert_eq!(
            block_on(operations[1 - index].apply(&adapter)),
            Err(PortError::QuotaExceeded)
        );
        assert_eq!(
            write_snapshot::snapshot(&inspection, &TABLES),
            references[index].state
        );
    }
    let reopened = SqliteProcessStore::open(&path, limits, clock(), 10, 16).unwrap();
    block_on(operations[winner].apply(&reopened)).unwrap();
    assert_eq!(
        write_snapshot::snapshot(&connection, &TABLES),
        references[winner].state
    );
    assert_eq!(
        block_on(operations[1 - winner].apply(&reopened)),
        Err(PortError::QuotaExceeded)
    );
    assert_eq!(
        write_snapshot::snapshot(&connection, &TABLES),
        references[winner].state
    );
    println!(
        "kind={kind} case={name} winner={winner} exact_state=true serial_rejections=2 reopened_retry=true"
    );
}
#[test]
fn concurrent_ports_share_the_exact_process_byte_budget() {
    let scope = scope();
    let mut other_definition = scope.clone();
    other_definition.definition_digest = ContentDigest([7; 32]);
    run_case(
        "bytes",
        "payload-payload-cross-definition",
        Write::Payload(Box::new(scope.clone()), [1; 16]),
        Write::Payload(Box::new(other_definition), [2; 16]),
        None,
    );
    run_case(
        "bytes",
        "inbox-inbox",
        input(&scope, "inp_left"),
        input(&scope, "inp_right"),
        None,
    );
    run_case(
        "bytes",
        "payload-inbox",
        Write::Payload(Box::new(scope.clone()), [1; 16]),
        input(&scope, "inp_racer"),
        None,
    );
    run_case(
        "bytes",
        "payload-commit",
        Write::Payload(Box::new(scope.clone()), [1; 16]),
        Write::Commit(Box::new(scoped_commit(&scope, false))),
        None,
    );
    let committed = scoped_commit(&scope, true);
    let timer = Write::Timer(Box::new(TimerSchedule {
        action: committed.actions[0].clone(),
        due_at: LogicalTime(100),
    }));
    let seed = Write::Commit(Box::new(committed));
    run_case(
        "bytes",
        "payload-timer",
        Write::Payload(Box::new(scope), [1; 16]),
        timer,
        Some(&seed),
    );
}
#[test]
fn concurrent_new_processes_share_the_scope_admission_limit() {
    let mut left = scope();
    left.process_id = id("prc_left");
    let mut right = scope();
    right.process_id = id("prc_right");
    run_case(
        "scopes",
        "payload-payload",
        Write::Payload(Box::new(left.clone()), [1; 16]),
        Write::Payload(Box::new(right.clone()), [2; 16]),
        None,
    );
    run_case(
        "scopes",
        "inbox-inbox",
        input(&left, "inp_left"),
        input(&right, "inp_right"),
        None,
    );
    run_case(
        "scopes",
        "payload-inbox",
        Write::Payload(Box::new(left.clone()), [1; 16]),
        input(&right, "inp_right"),
        None,
    );
    run_case(
        "scopes",
        "payload-commit",
        Write::Payload(Box::new(left), [1; 16]),
        Write::Commit(Box::new(scoped_commit(&right, false))),
        None,
    );
}
