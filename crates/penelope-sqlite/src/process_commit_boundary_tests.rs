//! Deferred constraints distinguish COMMIT failure from an earlier statement abort.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use super::*;
use penelope_domain::*;
use rusqlite::functions::FunctionFlags;
use std::{
    future::Future,
    num::NonZeroU16,
    sync::Mutex,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
#[path = "../tests/support/write_snapshot.rs"]
mod write_snapshot;
const TABLES: [&str; 12] = [
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
    "boundary_parent",
    "boundary_child",
];
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "commit-boundary operation timed out"
        );
        std::thread::park_timeout(Duration::from_millis(5));
    }
}
fn batch(long: bool, base: u64, count: u64, input: bool) -> AtomicProcessCommit {
    let suffix = if long {
        "x".repeat(124)
    } else {
        "audit".to_owned()
    };
    let scope = ProcessScope::new(
        format!("tnt_{suffix}").parse().unwrap(),
        format!("prc_{suffix}").parse().unwrap(),
        format!("def_{suffix}").parse().unwrap(),
        format!("dfv_{suffix}").parse().unwrap(),
        ContentDigest([9; 32]),
    );
    let input = input.then(|| {
        ProcessInput::new(
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            format!("inp_{base:04}").parse().unwrap(),
            ProcessInputKind::CanonicalEvent,
            ContentDigest([1; 32]),
        )
    });
    let mut outcomes = Vec::new();
    let mut actions = Vec::new();
    for offset in 0..count {
        let sequence = base + offset;
        outcomes.push(ProcessOutcome::new(
            scope.clone(),
            sequence,
            ProcessOutcomeFact::new(
                format!("out_{sequence:04}").parse().unwrap(),
                CausationId::Input(format!("inp_{base:04}").parse().unwrap()),
                OutcomeActor::System,
                LogicalTime(sequence),
                ProcessOutcomeKind::ActionPlanned,
                ContentDigest([2; 32]),
            ),
        ));
        if input.is_some() {
            actions.push(ProcessAction::new(
                scope.clone(),
                format!("act_{sequence:04}").parse().unwrap(),
                format!("stp_{sequence:04}").parse().unwrap(),
                0,
                ProcessActionKind::ExternalEffect,
                ContentDigest([3; 32]),
            ));
        }
    }
    let commit = AtomicProcessCommit::new(base, input, outcomes, actions).unwrap();
    if commit.input.is_some() {
        commit.with_canonical_source_event(format!("cev_{base:04}").parse().unwrap())
    } else {
        commit
    }
}
fn adapter(path: &Path, stages: Arc<Mutex<Vec<i64>>>) -> SqliteProcessStore {
    let connection = open(path, SqliteLimits::default()).unwrap();
    assert!(
        connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))
            .unwrap()
    );
    connection.execute_batch("CREATE TABLE boundary_parent(value INTEGER PRIMARY KEY); CREATE TABLE boundary_child(value INTEGER REFERENCES boundary_parent(value) DEFERRABLE INITIALLY DEFERRED)").unwrap();
    connection
        .create_scalar_function(
            "boundary_stage",
            1,
            FunctionFlags::SQLITE_UTF8,
            move |context| {
                let stage = context.get::<i64>(0)?;
                stages.lock().unwrap().push(stage);
                Ok(stage)
            },
        )
        .unwrap();
    SqliteProcessStore {
        worker: DatabaseWorker::new(connection, 16).unwrap(),
        limits: SqliteLimits::default(),
        clock: Arc::new(Clock),
        lease_millis: 10,
    }
}
fn arm(connection: &Connection, inbox_only: bool) {
    let table = if inbox_only {
        "process_inbox"
    } else {
        "process_commits"
    };
    let stage = if inbox_only { 3 } else { 1 };
    connection.execute_batch(&format!("CREATE TRIGGER boundary_fail AFTER INSERT ON {table} BEGIN INSERT INTO boundary_child VALUES(7); SELECT boundary_stage({stage}); END;")).unwrap();
    if !inbox_only {
        connection.execute_batch("CREATE TRIGGER boundary_consumed AFTER UPDATE OF commit_digest ON process_inbox BEGIN SELECT boundary_stage(2); END").unwrap();
    }
}
fn assert_clean(adapter: &SqliteProcessStore) {
    assert!(
        wait(
            adapter
                .worker
                .run(|connection| Ok(connection.is_autocommit()))
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        wait(
            adapter
                .worker
                .run(|connection| {
                    connection
                        .query_row("SELECT count(*) FROM boundary_child", [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .map_err(|error| db_error(&error))
                })
                .unwrap()
        ),
        Ok(0)
    );
}
#[test]
fn deferred_commit_failure_rolls_back_all_writes_and_preserves_public_retry() {
    let mut cases = 0;
    for long in [false, true] {
        for seeded in [false, true] {
            for preaccepted in [false, true] {
                for count in [1, 128] {
                    let directory = tempfile::tempdir().unwrap();
                    let path = directory.path().join("commit-boundary.db");
                    let stages = Arc::new(Mutex::new(Vec::new()));
                    let adapter = adapter(&path, stages.clone());
                    if seeded {
                        wait(ProcessStore::commit(&adapter, &batch(long, 0, 1, true))).unwrap();
                    }
                    let committed = batch(long, u64::from(seeded), count, true);
                    if preaccepted {
                        assert!(
                            !wait(adapter.accept(committed.input.as_ref().unwrap()))
                                .unwrap()
                                .duplicate
                        );
                    }
                    let connection = Connection::open(&path).unwrap();
                    let before = write_snapshot::snapshot(&connection, &TABLES);
                    arm(&connection, false);
                    assert_eq!(
                        wait(ProcessStore::commit(&adapter, &committed)),
                        Err(PortError::Conflict)
                    );
                    assert_eq!(
                        *stages.lock().unwrap(),
                        vec![1, 2],
                        "all writes must finish before COMMIT fails"
                    );
                    assert_clean(&adapter);
                    assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
                    connection
                        .execute_batch("DROP TRIGGER boundary_fail; DROP TRIGGER boundary_consumed")
                        .unwrap();
                    let receipt = wait(ProcessStore::commit(&adapter, &committed)).unwrap();
                    receipt.validate_for(&committed).unwrap();
                    assert_eq!(receipt.duplicate_input, preaccepted);
                    let after = write_snapshot::snapshot(&connection, &TABLES);
                    drop(adapter);
                    let reopened = SqliteProcessStore::open(
                        &path,
                        SqliteLimits::default(),
                        Arc::new(Clock),
                        10,
                        16,
                    )
                    .unwrap();
                    let duplicate = wait(ProcessStore::commit(&reopened, &committed)).unwrap();
                    duplicate.validate_for(&committed).unwrap();
                    assert!(duplicate.duplicate_input);
                    assert_eq!(write_snapshot::snapshot(&connection, &TABLES), after);
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 16);
    println!(
        "deferred_atomic_commit_cases={cases} final_consumption_observed=true rollback=true same_worker_retry=true reopen=true"
    );
}
#[test]
fn deferred_outcome_append_failure_rolls_back_and_preserves_replay() {
    let mut cases = 0;
    for long in [false, true] {
        for seeded in [false, true] {
            for count in [1, 128] {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("append-boundary.db");
                let stages = Arc::new(Mutex::new(Vec::new()));
                let adapter = adapter(&path, stages.clone());
                if seeded {
                    wait(ProcessStore::commit(&adapter, &batch(long, 0, 1, true))).unwrap();
                }
                let committed = batch(long, u64::from(seeded), count, false);
                let scope = committed.outcomes.first().unwrap().scope();
                let connection = Connection::open(&path).unwrap();
                let before = write_snapshot::snapshot(&connection, &TABLES);
                arm(&connection, false);
                assert_eq!(
                    wait(adapter.append_outcomes(
                        &scope,
                        committed.expected_sequence,
                        &committed.outcomes
                    )),
                    Err(PortError::Conflict)
                );
                assert_eq!(*stages.lock().unwrap(), vec![1]);
                assert_clean(&adapter);
                assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
                connection
                    .execute_batch("DROP TRIGGER boundary_fail; DROP TRIGGER boundary_consumed")
                    .unwrap();
                wait(adapter.append_outcomes(
                    &scope,
                    committed.expected_sequence,
                    &committed.outcomes,
                ))
                .unwrap();
                let after = write_snapshot::snapshot(&connection, &TABLES);
                drop(adapter);
                let reopened = SqliteProcessStore::open(
                    &path,
                    SqliteLimits::default(),
                    Arc::new(Clock),
                    10,
                    16,
                )
                .unwrap();
                wait(reopened.append_outcomes(
                    &scope,
                    committed.expected_sequence,
                    &committed.outcomes,
                ))
                .unwrap();
                let request = OutcomeReplayRequest::new(
                    scope,
                    committed.expected_sequence,
                    NonZeroU16::new(128).unwrap(),
                )
                .unwrap();
                let page = wait(reopened.read_outcomes(&request)).unwrap();
                assert_eq!(page.outcomes, committed.outcomes);
                assert_eq!(page.next_sequence, None);
                assert_eq!(write_snapshot::snapshot(&connection, &TABLES), after);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 8);
    println!(
        "deferred_append_cases={cases} receipt_insert_observed=true rollback=true same_worker_retry=true replay=true reopen=true"
    );
}
#[test]
fn deferred_inbox_failure_rolls_back_admission_and_usage_before_retry() {
    let mut cases = 0;
    for long in [false, true] {
        for seeded in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("inbox-boundary.db");
            let stages = Arc::new(Mutex::new(Vec::new()));
            let adapter = adapter(&path, stages.clone());
            if seeded {
                wait(ProcessStore::commit(&adapter, &batch(long, 0, 1, true))).unwrap();
            }
            let committed = batch(long, u64::from(seeded), 1, true);
            let input = committed.input.as_ref().unwrap();
            let connection = Connection::open(&path).unwrap();
            let before = write_snapshot::snapshot(&connection, &TABLES);
            arm(&connection, true);
            assert_eq!(wait(adapter.accept(input)), Err(PortError::Conflict));
            assert_eq!(*stages.lock().unwrap(), vec![3]);
            assert_clean(&adapter);
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), before);
            connection
                .execute_batch("DROP TRIGGER boundary_fail")
                .unwrap();
            assert!(!wait(adapter.accept(input)).unwrap().duplicate);
            let after = write_snapshot::snapshot(&connection, &TABLES);
            drop(adapter);
            let reopened =
                SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 10, 16)
                    .unwrap();
            assert!(wait(reopened.accept(input)).unwrap().duplicate);
            assert_eq!(write_snapshot::snapshot(&connection, &TABLES), after);
            cases += 1;
        }
    }
    assert_eq!(cases, 4);
    println!(
        "deferred_inbox_cases={cases} final_insert_observed=true rollback=true same_worker_retry=true reopen=true"
    );
}
