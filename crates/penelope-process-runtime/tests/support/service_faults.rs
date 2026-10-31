//! Validate entire adapter batches before delivery and preserve ambiguous writes.
use super::*;
use penelope_executor::services::{DispatchStage, dispatch_outbox};
use penelope_ports::payloads::ProcessPayloadStore;

struct TimerBatch {
    leases: Vec<TimerLease>,
    acknowledgements: AtomicU64,
}
#[async_trait]
impl TimerClaimStore for TimerBatch {
    async fn claim_due(&self, _request: &TimerClaimRequest) -> Result<Vec<TimerLease>, PortError> {
        Ok(self.leases.clone())
    }
    async fn acknowledge(&self, _lease: &TimerLease, _now: LogicalTime) -> Result<(), PortError> {
        self.acknowledgements.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
struct OutboxBatch {
    leases: Vec<OutboxLease>,
    acknowledgements: AtomicU64,
}
#[async_trait]
impl OutboxStore for OutboxBatch {
    async fn claim(&self, _request: &OutboxClaimRequest) -> Result<Vec<OutboxLease>, PortError> {
        Ok(self.leases.clone())
    }
    async fn acknowledge(&self, _lease: &OutboxLease, _now: LogicalTime) -> Result<(), PortError> {
        self.acknowledgements.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn renew(
        &self,
        _lease: &OutboxLease,
        _now: LogicalTime,
        _expiry: LogicalTime,
    ) -> Result<OutboxLease, PortError> {
        Err(PortError::Invariant)
    }
}
struct Dispatcher(AtomicU64);
#[async_trait]
impl ActionDispatcher for Dispatcher {
    async fn dispatch(&self, action: &ProcessAction) -> Result<ActionDispatchReceipt, PortError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ActionDispatchReceipt::new(
            action.scope(),
            action.action_id.clone(),
            false,
        ))
    }
}
struct InboxFault<'store> {
    store: &'store SqliteProcessStore,
    mode: u8,
    writes: AtomicU64,
    accepts: AtomicU64,
}
#[async_trait]
impl ProcessPayloadStore for InboxFault<'_> {
    async fn put_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
        bytes: &[u8],
    ) -> Result<ContentDigest, PortError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        let stored = self.store.put_payload(scope, digest, bytes).await?;
        match self.mode {
            0 => Err(PortError::Unavailable),
            1 => Ok(ContentDigest([9; 32])),
            _ => Ok(stored),
        }
    }
    async fn read_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
    ) -> Result<Vec<u8>, PortError> {
        self.store.read_payload(scope, digest).await
    }
}
#[async_trait]
impl Inbox for InboxFault<'_> {
    async fn accept(&self, input: &ProcessInput) -> Result<InboxAcceptanceReceipt, PortError> {
        self.accepts.fetch_add(1, Ordering::SeqCst);
        let mut receipt = self.store.accept(input).await?;
        if self.mode == 2 {
            return Err(PortError::Unavailable);
        }
        if self.mode == 3 {
            receipt.input_id = id("inp_substituted");
        }
        Ok(receipt)
    }
}
const fn policy() -> Policy {
    Policy {
        allow: AtomicBool::new(true),
        requests: Mutex::new(Vec::new()),
    }
}
fn timer_request(stores: &Stores) -> TimerClaimRequest {
    TimerClaimRequest::new(
        stores.scope.clone(),
        id("pri_timers"),
        LogicalTime(10),
        NonZeroU16::new(2).unwrap(),
    )
    .unwrap()
}

#[test]
fn malformed_timer_batches_never_write_payloads_accept_inputs_or_acknowledge() {
    for case in 0..5 {
        let stores = Stores::new(true, scope());
        scheduled(&stores);
        let request = timer_request(&stores);
        let lease = run(stores.store.claim_due(&request))
            .unwrap()
            .pop()
            .unwrap();
        let mut leases = vec![lease.clone(), lease];
        if case >= 3 {
            let changed = leases.last_mut().unwrap();
            changed.timer.action.action_id = id("act_other");
            changed.token = OutboxLeaseToken::new(std::num::NonZeroU64::new(9999).unwrap());
        }
        match case {
            0 => leases.push(leases.first().unwrap().clone()), // exceeds bound
            1 => {}                                            // same action and token
            2 => leases.last_mut().unwrap().timer.action.action_id = id("act_other"), // duplicate token
            3 => leases.last_mut().unwrap().owner = id("pri_foreign"),
            _ => leases.last_mut().unwrap().timer.action.tenant_id = id("tnt_foreign"),
        }
        let timers = TimerBatch {
            leases,
            acknowledgements: AtomicU64::new(0),
        };
        let inbox = InboxFault {
            store: &stores.store,
            mode: 4,
            writes: AtomicU64::new(0),
            accepts: AtomicU64::new(0),
        };
        let failure = run(drain_timers(
            &policy(),
            &timers,
            &inbox,
            &request,
            &Clock(AtomicU64::new(10)),
        ))
        .unwrap_err();
        assert_eq!(failure.stage, TimerDrainStage::Validate);
        assert_eq!(inbox.writes.load(Ordering::SeqCst), 0);
        assert_eq!(inbox.accepts.load(Ordering::SeqCst), 0);
        assert_eq!(timers.acknowledgements.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn malformed_outbox_batches_never_dispatch_or_acknowledge() {
    for case in 0..6 {
        let stores = Stores::new(false, scope());
        planned(&stores, ProcessActionKind::ExternalEffect);
        let request = OutboxClaimRequest::new(
            stores.scope.clone(),
            id("pri_dispatch"),
            NonZeroU16::new(2).unwrap(),
        )
        .unwrap();
        let lease = run(stores.store.claim(&request)).unwrap().pop().unwrap();
        let mut leases = vec![lease.clone(), lease];
        if case >= 3 {
            let changed = leases.last_mut().unwrap();
            changed.record.action.action_id = id("act_other");
            changed.token = OutboxLeaseToken::new(std::num::NonZeroU64::new(9999).unwrap());
        }
        match case {
            0 => leases.push(leases.first().unwrap().clone()),
            1 => {}
            2 => leases.last_mut().unwrap().record.action.action_id = id("act_other"),
            3 => leases.last_mut().unwrap().owner = id("pri_foreign"),
            4 => leases.last_mut().unwrap().record.action.tenant_id = id("tnt_foreign"),
            _ => {
                leases.last_mut().unwrap().record.acknowledgement =
                    OutboxAcknowledgement::Acknowledged
            }
        }
        let outbox = OutboxBatch {
            leases,
            acknowledgements: AtomicU64::new(0),
        };
        let dispatcher = Dispatcher(AtomicU64::new(0));
        let failure = run(dispatch_outbox(
            &policy(),
            &outbox,
            &dispatcher,
            &request,
            &Clock(AtomicU64::new(1)),
        ))
        .unwrap_err();
        assert_eq!(failure.stage, DispatchStage::Claim);
        assert_eq!(failure.report.delivered, 0);
        assert_eq!(dispatcher.0.load(Ordering::SeqCst), 0);
        assert_eq!(outbox.acknowledgements.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn ambiguous_timer_payload_and_inbox_receipts_recover_without_a_second_identity() {
    for mode in 0..4 {
        let stores = Stores::new(true, scope());
        let timer = scheduled(&stores);
        let request = timer_request(&stores);
        let inbox = InboxFault {
            store: &stores.store,
            mode,
            writes: AtomicU64::new(0),
            accepts: AtomicU64::new(0),
        };
        let clock = Clock(AtomicU64::new(10));
        let failure = run(drain_timers(
            &policy(),
            &stores.store,
            &inbox,
            &request,
            &clock,
        ))
        .unwrap_err();
        assert_eq!(
            failure.stage,
            if mode < 2 {
                TimerDrainStage::Payload
            } else {
                TimerDrainStage::Inbox
            }
        );
        assert_eq!(failure.report.claimed, 1);
        assert_eq!(failure.report.acknowledged, 0);
        assert_eq!(inbox.accepts.load(Ordering::SeqCst), u64::from(mode >= 2));
        // Both adapters may have committed before returning the bad/lost receipt.
        // Reclaim only after expiry and repeat the same canonical delivery.
        clock.0.store(111, Ordering::SeqCst);
        let recovered = run(drain_timers(
            &policy(),
            &stores.store,
            &stores.store,
            &request,
            &clock,
        ))
        .unwrap();
        assert_eq!(recovered.acknowledged, 1);
        assert_eq!(recovered.duplicate_inputs, u16::from(mode >= 2));
        let bytes = timer.canonical_wire_bytes().unwrap();
        assert_eq!(
            run(stores
                .store
                .read_payload(&stores.scope, ContentDigest::sha256(&bytes)))
            .unwrap(),
            bytes
        );
        assert_eq!(
            run(drain_timers(
                &policy(),
                &stores.store,
                &stores.store,
                &request,
                &clock
            ))
            .unwrap()
            .claimed,
            0
        );
    }
}
