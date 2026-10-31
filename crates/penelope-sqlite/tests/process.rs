#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::*;
use rusqlite::Connection;
use std::{
    future::Future,
    num::NonZeroU16,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

#[path = "support/lease_contracts.rs"]
mod lease_contracts;

#[path = "support/outbox_race.rs"]
mod outbox_race;

#[path = "support/timer_cancel_race.rs"]
mod timer_cancel_race;

#[path = "support/replay_scope.rs"]
mod replay_scope;

#[path = "support/pending_inbox.rs"]
mod pending_inbox;

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(Instant::now() < deadline, "async operation timed out");
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
struct TestClock {
    now: AtomicU64,
    reactor: std::thread::ThreadId,
}
impl SqliteLeaseClock for TestClock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        if std::thread::current().id() == self.reactor {
            return Err(PortError::Invariant);
        }
        Ok(LogicalTime(self.now.load(Ordering::SeqCst)))
    }
}
fn clock() -> Arc<TestClock> {
    Arc::new(TestClock {
        now: AtomicU64::new(100),
        reactor: std::thread::current().id(),
    })
}
fn id<T: FromStr>(text: &str) -> T {
    text.parse().ok().unwrap()
}
fn scope() -> ProcessScope {
    ProcessScope::new(
        id("tnt_sqlite"),
        id("prc_sqlite"),
        id("def_sqlite"),
        id("dfv_one"),
        ContentDigest([9; 32]),
    )
}
fn commit(timer: bool) -> AtomicProcessCommit {
    let scope = scope();
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id("inp_start"),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([1; 32]),
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            id("out_start"),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::Started,
            ContentDigest([2; 32]),
        ),
    );
    let action = ProcessAction::new(
        scope,
        id("act_work"),
        id("stp_work"),
        0,
        if timer {
            ProcessActionKind::Timer
        } else {
            ProcessActionKind::ExternalEffect
        },
        ContentDigest([3; 32]),
    );
    AtomicProcessCommit::new(0, Some(input), vec![outcome], vec![action]).unwrap()
}
fn store(path: &std::path::Path, clock: Arc<TestClock>) -> SqliteProcessStore {
    SqliteProcessStore::open(path, SqliteLimits::default(), clock, 10, 16).unwrap()
}
fn replay() -> OutcomeReplayRequest {
    OutcomeReplayRequest::new(scope(), 0, NonZeroU16::new(10).unwrap()).unwrap()
}
fn claim() -> OutboxClaimRequest {
    OutboxClaimRequest::new(scope(), id("pri_worker"), NonZeroU16::new(10).unwrap()).unwrap()
}
fn timer_claim(now: u64) -> TimerClaimRequest {
    TimerClaimRequest::new(
        scope(),
        id("pri_worker"),
        LogicalTime(now),
        NonZeroU16::new(10).unwrap(),
    )
    .unwrap()
}

#[test]
fn durable_atomic_commit_inbox_outbox_and_exact_retry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let clock = clock();
    let store = store(&path, clock);
    let commit = commit(false);
    let receipt = block_on(ProcessStore::commit(&store, &commit)).unwrap();
    receipt.validate_for(&commit).unwrap();
    assert!(!receipt.duplicate_input);
    let duplicate = block_on(ProcessStore::commit(&store, &commit)).unwrap();
    assert!(duplicate.duplicate_input);
    duplicate.validate_for(&commit).unwrap();
    let page = block_on(store.read_outcomes(&replay())).unwrap();
    assert_eq!(page.outcomes, commit.outcomes);
    assert_eq!(page.next_sequence, None);
    let inbox = block_on(store.accept(commit.input.as_ref().unwrap())).unwrap();
    assert!(inbox.duplicate);
    let leases = block_on(store.claim(&claim())).unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].record.action, commit.actions[0]);
    block_on(OutboxStore::acknowledge(
        &store,
        &leases[0],
        LogicalTime(100),
    ))
    .unwrap();
    block_on(OutboxStore::acknowledge(
        &store,
        &leases[0],
        LogicalTime(100),
    ))
    .unwrap();
    assert!(block_on(store.claim(&claim())).unwrap().is_empty());
}

#[test]
fn outbox_expiry_redelivery_renewal_and_substitution_are_fenced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let clock = clock();
    let store = store(&path, clock.clone());
    block_on(ProcessStore::commit(&store, &commit(false))).unwrap();
    let first = block_on(store.claim(&claim())).unwrap().remove(0);
    assert!(block_on(store.claim(&claim())).unwrap().is_empty());
    clock.now.store(111, Ordering::SeqCst);
    let second = block_on(store.claim(&claim())).unwrap().remove(0);
    assert_ne!(first.token, second.token);
    assert_eq!(second.record.delivery_attempt, 1);
    assert_eq!(
        block_on(OutboxStore::acknowledge(&store, &first, LogicalTime(105))),
        Err(PortError::Conflict)
    );
    assert_eq!(
        block_on(OutboxStore::acknowledge(&store, &first, LogicalTime(111))),
        Err(PortError::TimedOut)
    );
    let renewed = block_on(store.renew(&second, LogicalTime(112), LogicalTime(150))).unwrap();
    assert_eq!(
        block_on(OutboxStore::acknowledge(&store, &second, LogicalTime(112))),
        Err(PortError::Conflict)
    );
    let mut forged = renewed.clone();
    forged.owner = id("pri_attacker");
    assert_eq!(
        block_on(OutboxStore::acknowledge(&store, &forged, LogicalTime(112))),
        Err(PortError::Conflict)
    );
    block_on(OutboxStore::acknowledge(&store, &renewed, LogicalTime(112))).unwrap();
}

#[test]
fn durable_timer_claims_redeliver_until_exact_acknowledgement() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    let commit = commit(true);
    let timer = TimerSchedule {
        action: commit.actions[0].clone(),
        due_at: LogicalTime(120),
    };
    assert_eq!(block_on(store.schedule(&timer)), Err(PortError::Conflict));
    block_on(ProcessStore::commit(&store, &commit)).unwrap();
    block_on(store.schedule(&timer)).unwrap();
    block_on(store.schedule(&timer)).unwrap();
    assert!(
        block_on(store.claim_due(&timer_claim(119)))
            .unwrap()
            .is_empty()
    );
    let first = block_on(store.claim_due(&timer_claim(120)))
        .unwrap()
        .remove(0);
    assert!(
        block_on(store.claim_due(&timer_claim(130)))
            .unwrap()
            .is_empty()
    );
    let second = block_on(store.claim_due(&timer_claim(131)))
        .unwrap()
        .remove(0);
    assert_ne!(first.token, second.token);
    assert_eq!(
        block_on(TimerClaimStore::acknowledge(
            &store,
            &first,
            LogicalTime(125)
        )),
        Err(PortError::Conflict)
    );
    block_on(TimerClaimStore::acknowledge(
        &store,
        &second,
        LogicalTime(132),
    ))
    .unwrap();
    block_on(TimerClaimStore::acknowledge(
        &store,
        &second,
        LogicalTime(132),
    ))
    .unwrap();
    assert!(
        block_on(store.claim_due(&timer_claim(200)))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn transaction_failure_cannot_leave_inbox_outcomes_or_actions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_action BEFORE INSERT ON process_actions BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert_eq!(
        block_on(ProcessStore::commit(&store, &commit(false))),
        Err(PortError::Conflict)
    );
    for table in [
        "process_inbox",
        "process_outcomes",
        "process_actions",
        "process_heads",
        "process_usage",
    ] {
        let count: u32 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[test]
fn canonical_source_cannot_advance_under_another_input_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    let mut first = commit(false);
    first.input.as_mut().unwrap().kind = ProcessInputKind::CanonicalEvent;
    first.canonical_source_event_id = Some(id("cev_source"));
    block_on(ProcessStore::commit(&store, &first)).unwrap();
    let mut second = first.clone();
    second.expected_sequence = 1;
    second.input.as_mut().unwrap().input_id = id("inp_duplicate");
    second.outcomes[0].sequence = 1;
    second.outcomes[0].outcome_id = id("out_duplicate");
    second.outcomes[0].causation_id = CausationId::Input(id("inp_duplicate"));
    second.actions.clear();
    assert_eq!(
        block_on(ProcessStore::commit(&store, &second)),
        Err(PortError::Conflict)
    );
    assert_eq!(
        block_on(store.read_outcomes(&replay()))
            .unwrap()
            .outcomes
            .len(),
        1
    );
    let connection = Connection::open(&path).unwrap();
    let count: u32 = connection
        .query_row("SELECT count(*) FROM process_inbox", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn locked_database_does_not_block_the_callers_future_poll() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    let commit = commit(false);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut future = Box::pin(ProcessStore::commit(&store, &commit));
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let before = Instant::now();
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert!(before.elapsed() < Duration::from_millis(100));
    connection.execute_batch("ROLLBACK").unwrap();
    block_on(future).unwrap();
}

#[test]
fn missing_or_corrupt_durable_outcomes_never_become_an_empty_log() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    block_on(ProcessStore::commit(&store, &commit(false))).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute("DELETE FROM process_outcomes", [])
        .unwrap();
    assert_eq!(
        block_on(store.read_outcomes(&replay())),
        Err(PortError::Invariant)
    );
}

#[test]
fn committed_receipt_cannot_mask_missing_authoritative_records() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    let commit = commit(false);
    block_on(ProcessStore::commit(&store, &commit)).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute("DELETE FROM process_outcomes", [])
        .unwrap();
    assert_eq!(
        block_on(ProcessStore::commit(&store, &commit)),
        Err(PortError::Invariant)
    );
}

#[test]
fn separate_connections_cannot_both_claim_one_outbox_action() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let first = store(&path, clock());
    let second = store(&path, clock());
    block_on(ProcessStore::commit(&first, &commit(false))).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let first_barrier = barrier.clone();
    let first_worker = std::thread::spawn(move || {
        first_barrier.wait();
        block_on(first.claim(&claim())).unwrap()
    });
    let second_worker = std::thread::spawn(move || {
        barrier.wait();
        block_on(second.claim(&claim())).unwrap()
    });
    let first_result = first_worker.join().unwrap();
    let second_result = second_worker.join().unwrap();
    assert_eq!(first_result.len() + second_result.len(), 1);
}

#[test]
fn receipt_reconstruction_after_reopen_does_not_duplicate_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let original = store(&path, clock());
    let commit = commit(false);
    block_on(ProcessStore::commit(&original, &commit)).unwrap();
    drop(original);
    let reopened = store(&path, clock());
    let receipt = block_on(ProcessStore::commit(&reopened, &commit)).unwrap();
    assert!(receipt.duplicate_input);
    receipt.validate_for(&commit).unwrap();
    assert_eq!(
        block_on(reopened.read_outcomes(&replay()))
            .unwrap()
            .outcomes,
        commit.outcomes
    );
    assert_eq!(block_on(reopened.claim(&claim())).unwrap().len(), 1);
}

#[test]
fn inbox_and_atomic_commit_share_bounded_usage_and_exact_payload_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    let commit = commit(false);
    let input = commit.input.as_ref().unwrap();
    assert!(!block_on(store.accept(input)).unwrap().duplicate);
    let mut forged = input.clone();
    forged.payload_digest = ContentDigest([8; 32]);
    assert_eq!(block_on(store.accept(&forged)), Err(PortError::Conflict));
    let receipt = block_on(ProcessStore::commit(&store, &commit)).unwrap();
    assert!(receipt.duplicate_input);
    assert_eq!(
        block_on(store.read_outcomes(&replay()))
            .unwrap()
            .outcomes
            .len(),
        1
    );
    let small_path = directory.path().join("small.db");
    let limited = SqliteProcessStore::open(
        &small_path,
        SqliteLimits {
            max_scope_bytes: 1,
            ..SqliteLimits::default()
        },
        clock(),
        10,
        1,
    )
    .unwrap();
    assert_eq!(
        block_on(limited.accept(input)),
        Err(PortError::QuotaExceeded)
    );
    assert_eq!(
        block_on(ProcessStore::commit(&limited, &commit)),
        Err(PortError::QuotaExceeded)
    );
}

#[test]
fn replay_payloads_survive_reopen_are_scoped_and_detect_corruption() {
    use penelope_ports::payloads::ProcessPayloadStore;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    let bytes = b"immutable saga event bytes";
    let digest = ContentDigest::sha256(bytes);
    assert_eq!(
        block_on(store.put_payload(&scope(), digest, bytes)).unwrap(),
        digest
    );
    // Repeat after losing an acknowledgement: no duplicate allocation/charge.
    assert_eq!(
        block_on(store.put_payload(&scope(), digest, bytes)).unwrap(),
        digest
    );
    assert_eq!(
        block_on(store.read_payload(&scope(), digest)).unwrap(),
        bytes
    );
    let mut other = scope();
    other.definition_digest = ContentDigest([7; 32]);
    assert_eq!(
        block_on(store.read_payload(&other, digest)),
        Err(PortError::Invariant)
    );
    assert_eq!(
        block_on(store.put_payload(&scope(), digest, b"changed")),
        Err(PortError::Invariant)
    );
    let reopened =
        SqliteProcessStore::open(&path, SqliteLimits::default(), clock(), 10, 8).unwrap();
    assert_eq!(
        block_on(reopened.read_payload(&scope(), digest)).unwrap(),
        bytes
    );
    let connection = Connection::open(&path).unwrap();
    let count: u32 = connection
        .query_row("SELECT count(*) FROM process_payloads", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    connection
        .execute(
            "UPDATE process_payloads SET bytes=?1",
            [b"corrupt".as_slice()],
        )
        .unwrap();
    assert_eq!(
        block_on(reopened.read_payload(&scope(), digest)),
        Err(PortError::Invariant)
    );
    assert_eq!(
        block_on(reopened.put_payload(&scope(), digest, bytes)),
        Err(PortError::Invariant)
    );
    connection
        .execute("DELETE FROM process_payloads", [])
        .unwrap();
    assert_eq!(
        block_on(reopened.read_payload(&scope(), digest)),
        Err(PortError::Invariant)
    );
}

#[test]
fn replay_payload_limits_are_atomic_and_checked_before_stored_copy() {
    use penelope_ports::payloads::ProcessPayloadStore;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let limits = SqliteLimits {
        max_blob_bytes: 16,
        max_scope_bytes: 256,
        ..SqliteLimits::default()
    };
    let store = SqliteProcessStore::open(&path, limits, clock(), 10, 8).unwrap();
    let bytes = [1; 16];
    let digest = ContentDigest::sha256(&bytes);
    block_on(store.put_payload(&scope(), digest, &bytes)).unwrap();
    let connection = Connection::open(&path).unwrap();
    let used: i64 = connection
        .query_row("SELECT bytes_used FROM process_usage", [], |row| row.get(0))
        .unwrap();
    block_on(store.put_payload(&scope(), digest, &bytes)).unwrap();
    let repeated_used: i64 = connection
        .query_row("SELECT bytes_used FROM process_usage", [], |row| row.get(0))
        .unwrap();
    assert_eq!(used, repeated_used);
    let second = [3; 16];
    assert_eq!(
        block_on(store.put_payload(&scope(), ContentDigest::sha256(&second), &second)),
        Err(PortError::QuotaExceeded)
    );
    let over = [2; 17];
    assert_eq!(
        block_on(store.put_payload(&scope(), ContentDigest::sha256(&over), &over)),
        Err(PortError::QuotaExceeded)
    );
    // The row exists under the right key, but its length must be rejected before
    // allocating/copying stored bytes, even when an external writer changed it.
    connection
        .execute("UPDATE process_payloads SET bytes=zeroblob(1000000)", [])
        .unwrap();
    assert_eq!(
        block_on(store.read_payload(&scope(), digest)),
        Err(PortError::Invariant)
    );
    let count: u32 = connection
        .query_row("SELECT count(*) FROM process_payloads", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn version_three_upgrade_preserves_process_records_and_adds_payload_storage() {
    use penelope_ports::payloads::ProcessPayloadStore;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let store = store(&path, clock());
    let batch = commit(false);
    block_on(ProcessStore::commit(&store, &batch)).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("DROP INDEX process_actions_pending; DROP INDEX process_timers_due; DROP INDEX process_inbox_pending; DROP TABLE process_payloads; PRAGMA user_version=3;")
        .unwrap();
    let reopened =
        SqliteProcessStore::open(&path, SqliteLimits::default(), clock(), 10, 8).unwrap();
    assert_eq!(
        block_on(reopened.read_outcomes(&replay()))
            .unwrap()
            .outcomes,
        batch.outcomes
    );
    let bytes = b"event";
    let digest = ContentDigest::sha256(bytes);
    block_on(reopened.put_payload(&scope(), digest, bytes)).unwrap();
    assert_eq!(
        block_on(reopened.read_payload(&scope(), digest)).unwrap(),
        bytes
    );
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 6);
}

#[path = "support/commit_integrity.rs"]
mod commit_integrity;

#[path = "support/suppressed_process_writes.rs"]
mod suppressed_process_writes;
#[path = "support/write_snapshot.rs"]
mod write_snapshot;

#[path = "support/claim_indexes.rs"]
mod claim_indexes;

#[path = "support/record_limits.rs"]
mod record_limits;

#[path = "support/process_frontier.rs"]
mod process_frontier;

#[path = "support/replay_cache.rs"]
mod replay_cache;

#[path = "support/replay_columns.rs"]
mod replay_columns;

#[path = "support/inbox_index.rs"]
mod inbox_index;

#[path = "support/payload_columns.rs"]
mod payload_columns;

#[path = "support/lease_encoding.rs"]
mod lease_encoding;

#[path = "support/concurrent_quota.rs"]
mod concurrent_quota;

#[path = "support/timer_validation.rs"]
mod timer_validation;

#[path = "support/inbox_validation.rs"]
mod inbox_validation;

#[path = "support/inbox_concurrency.rs"]
mod inbox_concurrency;

#[path = "support/append_outcomes.rs"]
mod append_outcomes;

#[path = "support/commit_encoding.rs"]
mod commit_encoding;
