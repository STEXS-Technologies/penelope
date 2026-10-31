//! Typed outcome-only append fixture; construction and verification are uncounted.
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteProcessStore};
use std::{
    future::Future,
    num::NonZeroU16,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
pub(super) type Error = Box<dyn std::error::Error>;
pub(super) struct Clock;
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
pub(super) fn waker() -> Waker {
    Waker::from(Arc::new(ThreadWake(std::thread::current())))
}
pub(super) fn block_on<T>(future: impl Future<Output = T>) -> Result<T, Error> {
    let waker = waker();
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or("deadline overflow")?;
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return Ok(result);
        }
        if Instant::now() >= deadline {
            return Err("append timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(5));
    }
}
pub(super) struct Fixture {
    pub scope: ProcessScope,
    pub outcomes: Vec<ProcessOutcome>,
    pub checksum: ContentDigest,
}
impl Fixture {
    pub fn new(scope_len: usize, id_len: usize, count: usize) -> Result<Self, Error> {
        let suffix = "x".repeat(scope_len.checked_sub(4).ok_or("short scope")?);
        let padding = "x".repeat(id_len.checked_sub(8).ok_or("short identity")?);
        let scope = ProcessScope::new(
            format!("tnt_{suffix}").parse()?,
            format!("prc_{suffix}").parse()?,
            format!("def_{suffix}").parse()?,
            format!("dfv_{suffix}").parse()?,
            ContentDigest([9; 32]),
        );
        let mut outcomes = Vec::new();
        for sequence in 0..count {
            outcomes.push(ProcessOutcome::new(
                scope.clone(),
                u64::try_from(sequence)?,
                ProcessOutcomeFact::new(
                    format!("out_{padding}{sequence:04}").parse()?,
                    CausationId::Input(format!("inp_{padding}{sequence:04}").parse()?),
                    OutcomeActor::System,
                    LogicalTime(u64::try_from(sequence)?),
                    if sequence == 0 {
                        ProcessOutcomeKind::Started
                    } else {
                        ProcessOutcomeKind::ActionPlanned
                    },
                    ContentDigest([2; 32]),
                ),
            ));
        }
        let checksum = ContentDigest::sha256(&serde_json::to_vec(&outcomes)?);
        Ok(Self {
            scope,
            outcomes,
            checksum,
        })
    }
    pub fn verify(&self, store: &SqliteProcessStore) -> Result<(), Error> {
        let request = OutcomeReplayRequest::new(
            self.scope.clone(),
            0,
            NonZeroU16::new(128).ok_or("zero limit")?,
        )?;
        let page = block_on(store.read_outcomes(&request))??;
        if page.scope != self.scope
            || page.outcomes != self.outcomes
            || page.next_sequence.is_some()
        {
            return Err("append replay differs from fixture".into());
        }
        Ok(())
    }
}
