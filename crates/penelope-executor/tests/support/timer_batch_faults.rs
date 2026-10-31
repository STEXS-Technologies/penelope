//! Fail one real durable timer operation at a selected position in a claimed batch.
use async_trait::async_trait;
use penelope_domain::*;
use penelope_executor::services::TimerDrainStage;
use penelope_ports::{payloads::ProcessPayloadStore, *};
use penelope_sqlite::{SqliteLeaseClock, SqliteProcessStore};
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
pub struct Clock(pub AtomicU64);
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0.load(Ordering::SeqCst)))
    }
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
pub fn run<T>(future: impl Future<Output = T>) -> T {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    PayloadBefore,
    PayloadAfter,
    AcceptBefore,
    AcceptAfter,
    AckBefore,
    AckAfter,
    ClockAfterAccept,
    ReceiptMismatch,
}
impl Fault {
    pub const ALL: [Self; 8] = [
        Self::PayloadBefore,
        Self::PayloadAfter,
        Self::AcceptBefore,
        Self::AcceptAfter,
        Self::AckBefore,
        Self::AckAfter,
        Self::ClockAfterAccept,
        Self::ReceiptMismatch,
    ];
    pub const fn stage(self) -> TimerDrainStage {
        match self {
            Self::PayloadBefore | Self::PayloadAfter => TimerDrainStage::Payload,
            Self::AcceptBefore | Self::AcceptAfter | Self::ReceiptMismatch => {
                TimerDrainStage::Inbox
            }
            Self::AckBefore | Self::AckAfter => TimerDrainStage::Acknowledge,
            Self::ClockAfterAccept => TimerDrainStage::Clock,
        }
    }
    pub const fn accepted(self) -> bool {
        matches!(
            self,
            Self::AcceptAfter
                | Self::AckBefore
                | Self::AckAfter
                | Self::ClockAfterAccept
                | Self::ReceiptMismatch
        )
    }
}
pub struct Ports<'store> {
    store: &'store SqliteProcessStore,
    clock: &'store Clock,
    request: &'store TimerClaimRequest,
    fault: Option<(Fault, u64)>,
    pub fired: AtomicBool,
    pub payloads: AtomicU64,
    pub accepts: AtomicU64,
    pub acknowledgements: AtomicU64,
    pub clock_calls: AtomicU64,
    pub leases: Mutex<Vec<TimerLease>>,
}
impl<'store> Ports<'store> {
    pub const fn new(
        store: &'store SqliteProcessStore,
        clock: &'store Clock,
        request: &'store TimerClaimRequest,
        fault: Option<(Fault, u64)>,
    ) -> Self {
        Self {
            store,
            clock,
            request,
            fault,
            fired: AtomicBool::new(false),
            payloads: AtomicU64::new(0),
            accepts: AtomicU64::new(0),
            acknowledgements: AtomicU64::new(0),
            clock_calls: AtomicU64::new(0),
            leases: Mutex::new(Vec::new()),
        }
    }
    fn hit(&self, fault: Fault, index: u64) -> bool {
        let hit = self.fault == Some((fault, index));
        if hit {
            self.fired.store(true, Ordering::SeqCst);
        }
        hit
    }
}
#[async_trait]
impl ProcessAuthorizer for Ports<'_> {
    async fn authorize(
        &self,
        request: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        assert_eq!(request.scope, self.request.scope);
        assert_eq!(request.principal_id, self.request.owner);
        assert_eq!(
            request.operation,
            ProcessAuthorizationOperation::DeliverTimer
        );
        Ok(ProcessAuthorizationDecision::Authorized)
    }
}
#[async_trait]
impl penelope_ports::Clock for Ports<'_> {
    async fn now(&self) -> Result<LogicalTime, PortError> {
        let call = self.clock_calls.fetch_add(1, Ordering::SeqCst);
        if call > 0 && self.hit(Fault::ClockAfterAccept, call - 1) {
            return Err(PortError::Unavailable);
        }
        SqliteLeaseClock::now(self.clock)
    }
}
#[async_trait]
impl TimerClaimStore for Ports<'_> {
    async fn claim_due(&self, request: &TimerClaimRequest) -> Result<Vec<TimerLease>, PortError> {
        let leases = self.store.claim_due(request).await?;
        *self.leases.lock().unwrap() = leases.clone();
        Ok(leases)
    }
    async fn acknowledge(&self, lease: &TimerLease, now: LogicalTime) -> Result<(), PortError> {
        let index = self.acknowledgements.fetch_add(1, Ordering::SeqCst);
        if self.hit(Fault::AckBefore, index) {
            return Err(PortError::Unavailable);
        }
        TimerClaimStore::acknowledge(self.store, lease, now).await?;
        if self.hit(Fault::AckAfter, index) {
            return Err(PortError::Unavailable);
        }
        Ok(())
    }
}
#[async_trait]
impl ProcessPayloadStore for Ports<'_> {
    async fn put_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
        bytes: &[u8],
    ) -> Result<ContentDigest, PortError> {
        let index = self.payloads.fetch_add(1, Ordering::SeqCst);
        if self.hit(Fault::PayloadBefore, index) {
            return Err(PortError::Unavailable);
        }
        let stored = self.store.put_payload(scope, digest, bytes).await?;
        if self.hit(Fault::PayloadAfter, index) {
            return Err(PortError::Unavailable);
        }
        Ok(stored)
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
impl Inbox for Ports<'_> {
    async fn accept(&self, input: &ProcessInput) -> Result<InboxAcceptanceReceipt, PortError> {
        let index = self.accepts.fetch_add(1, Ordering::SeqCst);
        if self.hit(Fault::AcceptBefore, index) {
            return Err(PortError::Unavailable);
        }
        let mut receipt = self.store.accept(input).await?;
        if self.hit(Fault::AcceptAfter, index) {
            return Err(PortError::Unavailable);
        }
        if self.hit(Fault::ReceiptMismatch, index) {
            receipt.input_id = "inp_substituted".parse().unwrap();
        }
        Ok(receipt)
    }
}
