#![no_main]
//! Bounded operation sequences over real WAL/FULL storage. The model tracks
//! accepted/consumed input and durable outbox state independently of SQLite.
use libfuzzer_sys::fuzz_target;
use penelope_domain::*;
use penelope_ports::{inbox::PendingInbox, payloads::ProcessPayloadStore, *};
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    num::NonZeroU16,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn run<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(Instant::now() < deadline);
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
struct Clock(AtomicU64);
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0.load(Ordering::SeqCst)))
    }
}
fn id<T: std::str::FromStr>(text: &str) -> T {
    text.parse().ok().unwrap()
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.db");
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let limits = SqliteLimits {
        max_blob_bytes: 256,
        max_record_bytes: 4096,
        max_history_records: 64,
        max_scope_bytes: 65536,
        max_scopes: 4,
        max_database_bytes: 1024 * 1024,
        ..SqliteLimits::default()
    };
    let open = || SqliteProcessStore::open(&path, limits, clock.clone(), 10, 4).unwrap();
    let mut store = open();
    let scope = ProcessScope::new(
        id("tnt_fuzz"),
        id("prc_fuzz"),
        id("def_fuzz"),
        id("dfv_one"),
        ContentDigest([9; 32]),
    );
    let digest = ContentDigest::sha256(data);
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id("inp_fuzz"),
        ProcessInputKind::CanonicalEvent,
        digest,
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            id("out_fuzz"),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::Started,
            digest,
        ),
    );
    let action = ProcessAction::new(
        scope.clone(),
        id("act_fuzz"),
        id("stp_fuzz"),
        0,
        ProcessActionKind::ExternalEffect,
        digest,
    );
    let commit =
        AtomicProcessCommit::new(0, Some(input.clone()), vec![outcome], vec![action.clone()])
            .unwrap();
    let request =
        OutboxClaimRequest::new(scope.clone(), id("pri_fuzz"), NonZeroU16::new(1).unwrap())
            .unwrap();
    let replay = OutcomeReplayRequest::new(scope.clone(), 0, NonZeroU16::new(4).unwrap()).unwrap();
    let mut accepted = false;
    let mut committed = false;
    let mut acknowledged = false;
    let mut lease: Option<OutboxLease> = None;
    let mut attempt = 0;
    // The suffix visits every operation even with an empty input and verifies
    // an immutable retry after ACK cannot resurrect delivery.
    for operation in data
        .iter()
        .copied()
        .take(16)
        .chain([0, 1, 2, 3, 4, 5, 6, 7, 2, 3])
    {
        match operation % 8 {
            0 => {
                let receipt = run(store.accept(&input)).unwrap();
                assert_eq!(receipt.duplicate, accepted);
                accepted = true;
            }
            1 => {
                assert_eq!(
                    run(store.put_payload(&scope, digest, data)).unwrap(),
                    digest
                );
                assert_eq!(run(store.read_payload(&scope, digest)).unwrap(), data);
                let mut wrong = digest;
                wrong.0[0] ^= 1;
                assert_eq!(
                    run(store.put_payload(&scope, wrong, data)),
                    Err(PortError::Invariant)
                );
            }
            2 => {
                let receipt = run(ProcessStore::commit(&store, &commit)).unwrap();
                receipt.validate_for(&commit).unwrap();
                assert_eq!(receipt.duplicate_input, accepted);
                committed = true;
                accepted = true;
                // The same input cannot commit altered outcomes/actions, even
                // after its original outbox entry has been acknowledged.
                let mut altered = commit.clone();
                altered.actions[0].payload_digest.0[0] ^= 1;
                assert_eq!(
                    run(ProcessStore::commit(&store, &altered)),
                    Err(PortError::Conflict)
                );
            }
            3 => {
                let claimed = run(store.claim(&request)).unwrap();
                let now = clock.0.load(Ordering::SeqCst);
                let available = committed
                    && !acknowledged
                    && lease
                        .as_ref()
                        .is_none_or(|old| old.lease_expires_at.0 < now);
                assert_eq!(claimed.len(), usize::from(available));
                if let Some(next) = claimed.into_iter().next() {
                    assert_eq!(next.record.action, action);
                    assert_eq!(next.record.delivery_attempt, attempt);
                    if let Some(old) = &lease {
                        assert_ne!(old.token, next.token);
                    }
                    attempt += 1;
                    lease = Some(next);
                }
            }
            4 => {
                if let Some(current) = &lease {
                    let mut forged = current.clone();
                    forged.owner = id("pri_other");
                    assert!(
                        run(OutboxStore::acknowledge(
                            &store,
                            &forged,
                            clock.now().unwrap()
                        ))
                        .is_err()
                    );
                }
            }
            5 => {
                if let Some(current) = &lease {
                    let now = clock.now().unwrap();
                    let result = run(OutboxStore::acknowledge(&store, current, now));
                    if now <= current.lease_expires_at {
                        result.unwrap();
                        acknowledged = true;
                    } else {
                        assert_eq!(result, Err(PortError::TimedOut));
                    }
                }
            }
            6 => {
                clock
                    .0
                    .fetch_add(if operation & 8 == 0 { 10 } else { 11 }, Ordering::SeqCst);
            }
            _ => {
                drop(store);
                store = open();
            }
        }
        let page = run(store.read_outcomes(&replay)).unwrap();
        assert_eq!(
            page.outcomes,
            if committed {
                commit.outcomes.clone()
            } else {
                Vec::new()
            }
        );
        assert_eq!(page.next_sequence, None);
        let pending = run(store.pending_inputs(
            &scope.tenant_id,
            &scope.process_id,
            NonZeroU16::new(4).unwrap(),
        ))
        .unwrap();
        assert_eq!(
            pending,
            if accepted && !committed {
                vec![input.clone()]
            } else {
                Vec::new()
            }
        );
        let foreign: TenantId = id("tnt_other");
        assert!(
            run(store.pending_inputs(&foreign, &scope.process_id, NonZeroU16::new(4).unwrap()))
                .unwrap()
                .is_empty()
        );
    }
});
