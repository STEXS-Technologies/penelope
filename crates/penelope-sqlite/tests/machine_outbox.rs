//! Leases coordinate committed source handoff; receiver acceptance is separate.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use async_trait::async_trait;
use penelope_core::{machine::*, task::Digest};
use penelope_ports::machine::*;
use penelope_sqlite::{SqliteLimits, SqliteMachineOutbox, SqliteMachineStore};
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
#[path = "support/machines.rs"]
#[allow(dead_code)]
mod support;
use support::*;
struct Clock(AtomicU64);
impl MachineLeaseClock for Clock {
    fn now(&self) -> Result<MachineLeaseTick, MachineStoreError> {
        Ok(MachineLeaseTick::new(self.0.load(Ordering::SeqCst)))
    }
}
const LIMITS: MachineOutboxLimits = MachineOutboxLimits {
    lease_ticks: 10,
    max_claims: 2,
    max_claim_bytes: 1024,
    max_attempts: 3,
};
const fn request() -> MachineIntentClaim {
    MachineIntentClaim {
        scope: SCOPE,
        owner: MachineLeaseOwner::from_bytes([9; 32]),
        max_intents: 2,
        max_bytes: 1024,
    }
}
type Outbox = SqliteMachineOutbox<Policy, Clock, Receiver, 2>;
fn setup(
    path: &std::path::Path,
) -> (
    Arc<SqliteMachineStore<Policy, 2>>,
    Arc<Clock>,
    Outbox,
    Arc<Receiver>,
) {
    let store = Arc::new(open(path));
    block_on(store.bootstrap(&true, genesis())).unwrap();
    block_on(store.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let stored = block_on(store.lookup(&true, SCOPE, MachineEventId::from_bytes([4; 32])))
        .unwrap()
        .unwrap();
    let source = MachineIntentSource {
        head: stored.receipt().head,
        event: stored.receipt().event,
        commit: stored.receipt().identity,
        intent: *stored.batch().intents().next().unwrap().0,
    };
    let receiver = Arc::new(Receiver::from_source(source));
    let outbox = SqliteMachineOutbox::new(
        Arc::clone(&store),
        Arc::clone(&clock),
        Arc::clone(&receiver),
        LIMITS,
    )
    .unwrap();
    (store, clock, outbox, receiver)
}
/// Test receiver keeps its accepted native receipt independently of caller DTOs.
/// Production saga/task receivers will retain their own complete native receipts.
struct Receiver {
    source: MachineIntentSource,
    consumer: MachineSourceId,
    payload: [u8; 32],
    receipt: u64,
    calls: AtomicUsize,
    expire: std::sync::Mutex<Option<Arc<Clock>>>,
}
impl Receiver {
    fn from_source(source: MachineIntentSource) -> Self {
        Self {
            source,
            consumer: MachineSourceId::from_bytes([8; 32]),
            payload: Digest::of(&13_u64.to_le_bytes()).0,
            receipt: 42,
            calls: AtomicUsize::new(0),
            expire: std::sync::Mutex::new(None),
        }
    }
}
#[async_trait]
impl MachineIntentAcceptor for Receiver {
    type Receipt = u64;
    fn identity(&self) -> MachineSourceId {
        self.consumer
    }
    async fn verify(
        &self,
        source: &MachineIntentSource,
        payload: &[u8],
        receipt: &u64,
    ) -> Result<MachineIntentAcceptance, MachineStoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if *source != self.source
            || Digest::of(payload).0 != self.payload
            || *receipt != self.receipt
        {
            return Err(MachineStoreError::Conflict);
        }
        if let Some(clock) = &*self.expire.lock().unwrap() {
            clock.0.store(110, Ordering::SeqCst);
        }
        Ok(MachineIntentAcceptance {
            source: *source,
            consumer: self.consumer,
            receipt_digest: Digest::of(&receipt.to_be_bytes()).0,
        })
    }
}
#[test]
fn claim_is_source_bound_and_reopen_preserves_uncertainty_and_fencing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let (store, clock, outbox, receiver) = setup(&path);
    let first = block_on(outbox.claim(&true, request())).unwrap().remove(0);
    assert!(!first.requires_reconciliation());
    assert_eq!(first.payload(), 13_u64.to_le_bytes());
    let source = first.metadata().source;
    assert_eq!(source.head.revision, MachineRevision::new(1));
    assert_eq!(
        source.commit,
        block_on(store.lookup(&true, SCOPE, source.event))
            .unwrap()
            .unwrap()
            .receipt()
            .identity
    );
    block_on(outbox.validate(&true, &first)).unwrap();
    assert!(block_on(outbox.claim(&true, request())).unwrap().is_empty());
    drop(outbox);
    drop(store);
    clock.0.store(110, Ordering::SeqCst);
    let reopened = Arc::new(open(&path));
    let reopened_outbox = SqliteMachineOutbox::new(
        Arc::clone(&reopened),
        Arc::clone(&clock),
        Arc::clone(&receiver),
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        block_on(reopened_outbox.validate(&true, &first)),
        Err(MachineStoreError::Conflict)
    );
    let second = block_on(reopened_outbox.claim(&true, request()))
        .unwrap()
        .remove(0);
    assert!(second.requires_reconciliation());
    assert_eq!(second.metadata().attempt.get(), 2);
    assert_eq!(second.metadata().source, source);
    assert!(second.metadata().token > first.metadata().token);
    assert_eq!(
        block_on(reopened_outbox.validate(&true, &first)),
        Err(MachineStoreError::Conflict)
    );
    block_on(reopened_outbox.validate(&true, &second)).unwrap();
    assert_eq!(
        block_on(reopened_outbox.acknowledge(&true, &first, &42)),
        Err(MachineStoreError::Conflict)
    );
    assert_eq!(receiver.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn downstream_acceptance_lost_ack_is_reconstructed_and_exact_retry_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let (store, clock, outbox, receiver) = setup(&path);
    let lease = block_on(outbox.claim(&true, request())).unwrap().remove(0);
    assert_eq!(
        block_on(outbox.acknowledge(&true, &lease, &41)),
        Err(MachineStoreError::Conflict)
    );
    let accepted = block_on(outbox.acknowledge(&true, &lease, &42)).unwrap();
    drop(outbox);
    drop(store);
    clock.0.store(120, Ordering::SeqCst);
    let reopened_outbox = SqliteMachineOutbox::new(
        Arc::new(open(&path)),
        Arc::clone(&clock),
        Arc::clone(&receiver),
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        block_on(reopened_outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
            .unwrap()
            .unwrap()
            .accepted,
        Some(accepted)
    );
    assert_eq!(
        block_on(reopened_outbox.acknowledge(&true, &lease, &42)).unwrap(),
        accepted
    );
    assert!(
        block_on(reopened_outbox.claim(&true, request()))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        block_on(reopened_outbox.validate(&true, &lease)),
        Err(MachineStoreError::Conflict)
    );
    let mut other = Receiver::from_source(lease.metadata().source);
    other.consumer = MachineSourceId::from_bytes([7; 32]);
    let other_outbox = SqliteMachineOutbox::new(
        Arc::new(open(&path)),
        Arc::clone(&clock),
        Arc::new(other),
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        block_on(other_outbox.acknowledge(&true, &lease, &42)),
        Err(MachineStoreError::Conflict)
    );
}
#[test]
fn policy_denies_claim_read_validation_and_duplicate_ack_before_receipt_lookup() {
    let directory = tempfile::tempdir().unwrap();
    let (_, _, outbox, receiver) = setup(&directory.path().join("outbox.db"));
    let lease = block_on(outbox.claim(&true, request())).unwrap().remove(0);
    block_on(outbox.acknowledge(&true, &lease, &42)).unwrap();
    assert_eq!(
        block_on(outbox.claim(&false, request())).err(),
        Some(MachineStoreError::Denied)
    );
    assert_eq!(
        block_on(outbox.status(&false, SCOPE, lease.metadata().source.intent.identity)),
        Err(MachineStoreError::Denied)
    );
    assert_eq!(
        block_on(outbox.validate(&false, &lease)),
        Err(MachineStoreError::Denied)
    );
    assert_eq!(
        block_on(outbox.acknowledge(&false, &lease, &42)),
        Err(MachineStoreError::Denied)
    );
    assert_eq!(receiver.calls.load(Ordering::SeqCst), 1);
}
#[test]
fn acknowledgement_rechecks_expiry_after_native_verifier_returns() {
    let directory = tempfile::tempdir().unwrap();
    let (_, clock, outbox, receiver) = setup(&directory.path().join("outbox.db"));
    let lease = block_on(outbox.claim(&true, request())).unwrap().remove(0);
    *receiver.expire.lock().unwrap() = Some(clock);
    assert_eq!(
        block_on(outbox.acknowledge(&true, &lease, &42)),
        Err(MachineStoreError::Conflict)
    );
    let status = block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
        .unwrap()
        .unwrap();
    assert_eq!(status.accepted, None);
    assert_eq!(status.attempt.get(), 1);
    assert!(
        block_on(outbox.claim(&true, request()))
            .unwrap()
            .remove(0)
            .requires_reconciliation()
    );
}
#[test]
fn dto_reconstruction_cannot_substitute_lease_or_committed_source() {
    let directory = tempfile::tempdir().unwrap();
    let (_, _, outbox, _receiver) = setup(&directory.path().join("outbox.db"));
    let lease = block_on(outbox.claim(&true, request())).unwrap().remove(0);
    for change in 0..6 {
        let mut metadata = lease.metadata();
        match change {
            0 => metadata.owner = MachineLeaseOwner::from_bytes([5; 32]),
            1 => metadata.token = MachineLeaseToken::new(999),
            2 => metadata.expires = MachineLeaseTick::new(200),
            3 => metadata.source.commit = MachineCommitId::from_bytes([5; 32]),
            4 => metadata.source.head.revision = MachineRevision::new(2),
            _ => metadata.attempt = MachineDeliveryAttempt::new(2),
        }
        let forged = MachineIntentLease::decode(metadata, lease.payload(), 8).unwrap();
        assert_eq!(
            block_on(outbox.validate(&true, &forged)),
            Err(MachineStoreError::Conflict)
        );
    }
    assert_eq!(
        MachineIntentLease::decode(lease.metadata(), &[0; 8], 8).err(),
        Some(MachineStoreError::Corrupt)
    );
}
#[test]
fn claim_bounds_and_scoped_delivery_quota_rollback_without_consuming_tokens() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let store = Arc::new(
        SqliteMachineStore::<_, 2>::open(
            &path,
            SqliteLimits {
                max_scope_bytes: 744,
                ..SqliteLimits::default()
            },
            MachinePayloadLimits {
                max_batch_bytes: 744,
                ..PAYLOADS
            },
            Policy,
            8,
        )
        .unwrap(),
    );
    block_on(store.bootstrap(&true, genesis())).unwrap();
    block_on(store.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
    let stored = block_on(store.lookup(&true, SCOPE, MachineEventId::from_bytes([4; 32])))
        .unwrap()
        .unwrap();
    let receiver = Arc::new(Receiver::from_source(MachineIntentSource {
        head: stored.receipt().head,
        event: stored.receipt().event,
        commit: stored.receipt().identity,
        intent: *stored.batch().intents().next().unwrap().0,
    }));
    let outbox = SqliteMachineOutbox::new(
        Arc::clone(&store),
        Arc::new(Clock(AtomicU64::new(100))),
        receiver,
        MachineOutboxLimits {
            max_claim_bytes: 744,
            ..LIMITS
        },
    )
    .unwrap();
    let mut request = request();
    request.max_bytes = 744;
    assert_eq!(
        block_on(outbox.claim(&true, request)).err(),
        Some(MachineStoreError::Capacity)
    ); // 441 journal + 304 delivery.
    let connection = rusqlite::Connection::open(path).unwrap();
    let (count,token):(i64,i64)=connection.query_row("SELECT (SELECT count(*) FROM machine_intent_delivery),(SELECT next_token FROM lease_counter)",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
    assert_eq!((count, token), (0, 1));
    let mut too_small = request;
    too_small.max_bytes = 311;
    assert_eq!(
        block_on(outbox.claim(&true, too_small)).err(),
        Some(MachineStoreError::Capacity)
    );
}
#[test]
fn independent_workers_never_return_competing_live_leases() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let (_, clock, first, receiver) = setup(&path);
    let second =
        SqliteMachineOutbox::new(Arc::new(open(&path)), clock, Arc::clone(&receiver), LIMITS)
            .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let other = Arc::clone(&barrier);
    let a = std::thread::spawn(move || {
        barrier.wait();
        block_on(first.claim(&true, request())).unwrap()
    });
    let b = std::thread::spawn(move || {
        other.wait();
        block_on(second.claim(&true, request())).unwrap()
    });
    assert_eq!(a.join().unwrap().len() + b.join().unwrap().len(), 1);
}
#[test]
fn clock_regression_and_attempt_exhaustion_never_reset_uncertain_delivery() {
    let directory = tempfile::tempdir().unwrap();
    let (_, clock, outbox, _receiver) = setup(&directory.path().join("outbox.db"));
    let first = block_on(outbox.claim(&true, request())).unwrap().remove(0);
    clock.0.store(99, Ordering::SeqCst);
    assert_eq!(
        block_on(outbox.validate(&true, &first)),
        Err(MachineStoreError::Corrupt)
    );
    assert_eq!(
        block_on(outbox.claim(&true, request())).err(),
        Some(MachineStoreError::Corrupt)
    );
    for tick in [110, 120] {
        clock.0.store(tick, Ordering::SeqCst);
        assert!(
            block_on(outbox.claim(&true, request()))
                .unwrap()
                .remove(0)
                .requires_reconciliation()
        );
    }
    clock.0.store(130, Ordering::SeqCst);
    assert_eq!(
        block_on(outbox.claim(&true, request())).err(),
        Some(MachineStoreError::Capacity)
    );
    assert_eq!(
        block_on(outbox.status(&true, SCOPE, first.metadata().source.intent.identity))
            .unwrap()
            .unwrap()
            .attempt
            .get(),
        3
    );
}
#[test]
fn suppressed_or_substituted_delivery_writes_never_return_authority() {
    for body in [
        "SELECT RAISE(IGNORE);",
        "UPDATE machine_intent_delivery SET token=token+1 WHERE machine_key=new.machine_key AND intent=new.intent;",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outbox.db");
        let (_, _, outbox, _receiver) = setup(&path);
        let connection = rusqlite::Connection::open(path).unwrap();
        let timing = if body.starts_with("SELECT") {
            "BEFORE"
        } else {
            "AFTER"
        };
        connection.execute_batch(&format!("CREATE TRIGGER sabotage {timing} INSERT ON machine_intent_delivery BEGIN {body} END;")).unwrap();
        assert_eq!(
            block_on(outbox.claim(&true, request())).err(),
            Some(MachineStoreError::Corrupt)
        );
        let (count,token):(i64,i64)=connection.query_row("SELECT (SELECT count(*) FROM machine_intent_delivery),(SELECT next_token FROM lease_counter)",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!((count, token), (0, 1));
    }
}
#[test]
fn suppressed_ack_preserves_pending_source_and_reclaim_uncertainty() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let (_, clock, outbox, _receiver) = setup(&path);
    let lease = block_on(outbox.claim(&true, request())).unwrap().remove(0);
    let connection = rusqlite::Connection::open(path).unwrap();
    connection.execute_batch("CREATE TRIGGER sabotage BEFORE UPDATE ON machine_intent_delivery BEGIN SELECT RAISE(IGNORE); END;").unwrap();
    assert_eq!(
        block_on(outbox.acknowledge(&true, &lease, &42)),
        Err(MachineStoreError::Corrupt)
    );
    assert_eq!(
        block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
            .unwrap()
            .unwrap()
            .accepted,
        None
    );
    connection.execute_batch("DROP TRIGGER sabotage").unwrap();
    clock.0.store(110, Ordering::SeqCst);
    assert!(
        block_on(outbox.claim(&true, request()))
            .unwrap()
            .remove(0)
            .requires_reconciliation()
    );
}

#[test]
fn bounded_claim_keeps_revision_order_and_never_skips_the_next_intent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let (store, _, outbox, _receiver) = setup(&path);
    block_on(store.commit(&true, batch(&mut restore(&store), 1, 5))).unwrap();
    let mut request = request();
    request.max_bytes = 312;
    let first = block_on(outbox.claim(&true, request)).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].metadata().source.head.revision.get(), 1);
    let second = block_on(outbox.claim(&true, request)).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].metadata().source.head.revision.get(), 2);
    assert_eq!(second[0].payload(), 14_u64.to_le_bytes());
    assert_eq!(
        first[0].metadata().token.get() + 1,
        second[0].metadata().token.get()
    );
}
#[test]
fn second_claim_quota_failure_rolls_back_the_entire_prefix() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let (original, clock, _, receiver) = setup(&path);
    block_on(original.commit(&true, batch(&mut restore(&original), 1, 5))).unwrap();
    let store = Arc::new(
        SqliteMachineStore::<_, 2>::open(
            &path,
            SqliteLimits {
                max_scope_bytes: 1473,
                ..SqliteLimits::default()
            },
            PAYLOADS,
            Policy,
            8,
        )
        .unwrap(),
    );
    let outbox = SqliteMachineOutbox::new(store, clock, receiver, LIMITS).unwrap();
    assert_eq!(
        block_on(outbox.claim(&true, request())).err(),
        Some(MachineStoreError::Capacity)
    );
    let connection = rusqlite::Connection::open(path).unwrap();
    let (count,token,bytes):(i64,i64,i64)=connection.query_row("SELECT (SELECT count(*) FROM machine_intent_delivery),(SELECT next_token FROM lease_counter),delivery_bytes FROM machine_heads",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
    assert_eq!((count, token, bytes), (0, 1, 0));
}
#[test]
fn counter_substitution_exhaustion_and_deferred_commit_failure_return_no_lease() {
    for sabotage in [
        "CREATE TRIGGER sabotage AFTER UPDATE ON lease_counter BEGIN UPDATE lease_counter SET next_token=old.next_token; END;",
        "UPDATE lease_counter SET next_token=9223372036854775807;",
        "CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE deferred_failure(id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER sabotage AFTER INSERT ON machine_intent_delivery BEGIN INSERT INTO deferred_failure VALUES(99); END;",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outbox.db");
        let (_, _, outbox, _receiver) = setup(&path);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch(sabotage).unwrap();
        let expected = if sabotage.starts_with("UPDATE") {
            MachineStoreError::Capacity
        } else if sabotage.starts_with("CREATE TABLE") {
            MachineStoreError::Conflict
        } else {
            MachineStoreError::Corrupt
        };
        assert_eq!(
            block_on(outbox.claim(&true, request())).err(),
            Some(expected)
        );
        let count: i64 = connection
            .query_row("SELECT count(*) FROM machine_intent_delivery", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[test]
fn receiver_is_durably_pinned_before_admission_and_cannot_change_on_reclaim() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let (store, clock, outbox, receiver) = setup(&path);
    let first = block_on(outbox.claim(&true, request())).unwrap().remove(0);
    let mut changed = Receiver::from_source(first.metadata().source);
    changed.consumer = MachineSourceId::from_bytes([99; 32]);
    let changed = Arc::new(changed);
    let other = SqliteMachineOutbox::new(
        Arc::clone(&store),
        Arc::clone(&clock),
        Arc::clone(&changed),
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        block_on(other.validate(&true, &first)),
        Err(MachineStoreError::Conflict)
    );
    assert_eq!(
        block_on(other.acknowledge(&true, &first, &42)),
        Err(MachineStoreError::Conflict)
    );
    assert_eq!(changed.calls.load(Ordering::SeqCst), 0);
    clock.0.store(110, Ordering::SeqCst);
    drop(outbox);
    drop(other);
    drop(store);
    let reopened = Arc::new(open(&path));
    let reopened_other =
        SqliteMachineOutbox::new(Arc::clone(&reopened), Arc::clone(&clock), changed, LIMITS)
            .unwrap();
    assert!(matches!(
        block_on(reopened_other.claim(&true, request())),
        Err(MachineStoreError::Conflict)
    ));
    let original =
        SqliteMachineOutbox::new(reopened, clock, Arc::clone(&receiver), LIMITS).unwrap();
    let status = block_on(original.status(&true, SCOPE, first.metadata().source.intent.identity))
        .unwrap()
        .unwrap();
    assert_eq!(status.consumer, Some(receiver.consumer));
    assert_eq!(status.attempt.get(), 1);
    let next = block_on(original.claim(&true, request()))
        .unwrap()
        .remove(0);
    assert_eq!(next.metadata().attempt.get(), 2);
    block_on(original.acknowledge(&true, &next, &42)).unwrap();
}

#[test]
fn version8_pending_unknown_receiver_is_not_reinterpreted_and_accepted_receipts_survive() {
    for accepted in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outbox.db");
        let (store, clock, outbox, receiver) = setup(&path);
        let lease = block_on(outbox.claim(&true, request())).unwrap().remove(0);
        if accepted {
            block_on(outbox.acknowledge(&true, &lease, &42)).unwrap();
        }
        drop(outbox);
        drop(store);
        let sql = rusqlite::Connection::open(&path).unwrap();
        machine_schema::downgrade_to_v8(&sql);
        drop(sql);
        clock.0.store(110, Ordering::SeqCst);
        let upgraded_outbox =
            SqliteMachineOutbox::new(Arc::new(open(&path)), clock, Arc::clone(&receiver), LIMITS)
                .unwrap();
        let status =
            block_on(upgraded_outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
                .unwrap()
                .unwrap();
        assert_eq!(status.attempt.get(), 1);
        assert_eq!(status.accepted.is_some(), accepted);
        if accepted {
            assert_eq!(status.consumer, Some(receiver.consumer));
            block_on(upgraded_outbox.acknowledge(&true, &lease, &42)).unwrap();
            assert!(
                block_on(upgraded_outbox.claim(&true, request()))
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert_eq!(status.consumer, None);
            assert!(matches!(
                block_on(upgraded_outbox.claim(&true, request())),
                Err(MachineStoreError::Conflict)
            ));
            assert_eq!(
                block_on(upgraded_outbox.validate(&true, &lease)),
                Err(MachineStoreError::Conflict)
            );
            assert_eq!(
                block_on(upgraded_outbox.acknowledge(&true, &lease, &42)),
                Err(MachineStoreError::Conflict)
            );
            assert_eq!(receiver.calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[path = "support/machine_schema.rs"]
mod machine_schema;

#[path = "support/write_snapshot.rs"]
mod delivery_snapshot;
#[test]
fn malformed_delivery_rows_and_orphaned_sources_never_gain_admission_or_reset_uncertainty() {
    for damage in [
        "UPDATE machine_intent_delivery SET owner=printf('%032d',1)",
        "UPDATE machine_intent_delivery SET token=0",
        "UPDATE machine_intent_delivery SET expires=zeroblob(8)",
        "UPDATE machine_intent_delivery SET attempt=0",
        "UPDATE machine_intent_delivery SET consumer=printf('%032d',1)",
        "UPDATE machine_intent_delivery SET receipt=printf('%032d',1)",
        "UPDATE machine_intent_delivery SET consumer=NULL,receipt=zeroblob(32)",
        "DELETE FROM machine_intents",
        "DELETE FROM machine_events",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("damaged.db");
        let (_store, _clock, outbox, receiver) = setup(&path);
        let lease = block_on(outbox.claim(&true, request())).unwrap().remove(0);
        let sql = rusqlite::Connection::open(&path).unwrap();
        // Simulate storage corruption, including states CHECK normally rejects.
        sql.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA ignore_check_constraints=ON")
            .unwrap();
        sql.execute_batch(damage).unwrap();
        let tables = [
            "machine_heads",
            "machine_events",
            "machine_intents",
            "machine_intent_delivery",
            "lease_counter",
        ];
        let before = delivery_snapshot::snapshot(&sql, &tables);
        assert_eq!(
            block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity)),
            Err(MachineStoreError::Corrupt),
            "{damage}"
        );
        assert_eq!(
            block_on(outbox.validate(&true, &lease)),
            Err(MachineStoreError::Corrupt),
            "{damage}"
        );
        assert_eq!(
            block_on(outbox.acknowledge(&true, &lease, &42)),
            Err(MachineStoreError::Corrupt),
            "{damage}"
        );
        assert!(
            matches!(
                block_on(outbox.claim(&true, request())),
                Err(MachineStoreError::Corrupt)
            ),
            "{damage}"
        );
        assert_eq!(receiver.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            delivery_snapshot::snapshot(&sql, &tables),
            before,
            "{damage}"
        );
    }
}
