//! Public empty-queue claim latency, including worker dispatch and transactions.
use penelope_domain::{ContentDigest, LogicalTime, ProcessScope};
use penelope_ports::{
    OutboxClaimRequest, OutboxStore, PortError, TimerClaimRequest, TimerClaimStore,
};
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    num::NonZeroU16,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;
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
fn block_on<T>(future: impl Future<Output = T>) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
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
            return Err("claim timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
fn main() -> Result<(), Error> {
    let directory = tempfile::tempdir()?;
    let store = SqliteProcessStore::open(
        &directory.path().join("claims.db"),
        SqliteLimits::default(),
        Arc::new(Clock),
        100,
        16,
    )?;
    let scope = ProcessScope::new(
        "tnt_bench".parse()?,
        "prc_bench".parse()?,
        "def_bench".parse()?,
        "dfv_one".parse()?,
        ContentDigest([1; 32]),
    );
    let limit = NonZeroU16::new(10).ok_or("zero limit")?;
    let outbox = OutboxClaimRequest::new(scope.clone(), "pri_bench".parse()?, limit)?;
    let timer = TimerClaimRequest::new(scope, "pri_bench".parse()?, LogicalTime(100), limit)?;
    for timers in [false, true] {
        for _ in 0..100 {
            if timers {
                if !block_on(store.claim_due(&timer))??.is_empty() {
                    return Err("unexpected timer".into());
                }
            } else if !block_on(store.claim(&outbox))??.is_empty() {
                return Err("unexpected action".into());
            }
        }
        let iterations = 2_000_u32;
        let started = Instant::now();
        for _ in 0..iterations {
            if timers {
                if !block_on(store.claim_due(&timer))??.is_empty() {
                    return Err("unexpected timer".into());
                }
            } else if !block_on(store.claim(&outbox))??.is_empty() {
                return Err("unexpected action".into());
            }
        }
        println!(
            "timers={timers} iterations={iterations} elapsed_ns={}",
            started.elapsed().as_nanos()
        );
    }
    Ok(())
}
