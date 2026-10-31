//! Pending polls through the public async port; SQLite setup/work is untimed.
use penelope_domain::{ContentDigest, LogicalTime, ProcessScope};
use penelope_ports::{OutboxClaimRequest, OutboxStore, PortError};
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    hint::black_box,
    num::NonZeroU16,
    sync::{Arc, Mutex, mpsc},
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;
struct GateClock {
    entered: mpsc::SyncSender<()>,
    resume: Mutex<mpsc::Receiver<()>>,
}
impl SqliteLeaseClock for GateClock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        self.entered
            .send(())
            .map_err(|_closed| PortError::Unavailable)?;
        self.resume
            .lock()
            .map_err(|_poison| PortError::Unavailable)?
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_timeout| PortError::TimedOut)?;
        Ok(LogicalTime(1))
    }
}
struct Noop;
// Distinct allocations are required to measure changing registrations.
#[allow(clippy::manual_noop_waker)]
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}
fn main() -> Result<(), Error> {
    for alternate in [false, true] {
        let directory = tempfile::tempdir()?;
        let (entered, started) = mpsc::sync_channel(1);
        let (release, resume) = mpsc::sync_channel(1);
        let clock = Arc::new(GateClock {
            entered,
            resume: Mutex::new(resume),
        });
        let store = SqliteProcessStore::open(
            &directory.path().join("poll.db"),
            SqliteLimits::default(),
            clock,
            100,
            1,
        )?;
        let scope = ProcessScope::new(
            "tnt_poll".parse()?,
            "prc_poll".parse()?,
            "def_poll".parse()?,
            "dfv_one".parse()?,
            ContentDigest([1; 32]),
        );
        let request = OutboxClaimRequest::new(
            scope,
            "pri_poll".parse()?,
            NonZeroU16::new(1).ok_or("zero limit")?,
        )?;
        let wakers = [Waker::from(Arc::new(Noop)), Waker::from(Arc::new(Noop))];
        let mut future = Box::pin(store.claim(&request));
        let first = wakers.first().ok_or("missing waker")?;
        if future
            .as_mut()
            .poll(&mut Context::from_waker(first))
            .is_ready()
        {
            return Err("claim did not queue".into());
        }
        started.recv_timeout(Duration::from_secs(5))?;
        let iterations = 3_000_000_u32;
        let began = Instant::now();
        for index in 0..iterations {
            let which = if alternate { index % 2 } else { 0 };
            let waker = wakers.get(usize::try_from(which)?).ok_or("missing waker")?;
            if black_box(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(black_box(waker))),
            )
            .is_ready()
            {
                return Err("blocked worker completed".into());
            }
        }
        let elapsed = began.elapsed();
        release.send(())?;
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(5))
            .ok_or("deadline overflow")?;
        loop {
            if let Poll::Ready(result) = future.as_mut().poll(&mut Context::from_waker(first)) {
                if !result?.is_empty() {
                    return Err("unexpected leases".into());
                }
                break;
            }
            if Instant::now() >= deadline {
                return Err("completion timed out".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let nanos = elapsed
            .as_nanos()
            .checked_div(u128::from(iterations))
            .ok_or("zero iterations")?;
        println!(
            "sqlite pending alternate={alternate}: {iterations} operations in {elapsed:?} ({nanos} ns/op)"
        );
    }
    Ok(())
}
