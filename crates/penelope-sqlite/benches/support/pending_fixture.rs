//! Typed public pending-inbox fixture shared by ordinary and allocation probes.
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
pub(super) type Error = Box<dyn std::error::Error>;
struct Clock(AtomicU64);
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0.load(Ordering::SeqCst)))
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
pub(super) fn run(
    mut measure: impl FnMut(u16, &dyn Fn() -> Result<(), Error>) -> Result<(), Error>,
) -> Result<(), Error> {
    use penelope_ports::inbox::PendingInbox;
    for pending in [0_u16, 1, 16] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("pending.db");
        let store = SqliteProcessStore::open(
            &path,
            SqliteLimits::default(),
            Arc::new(Clock(AtomicU64::new(100))),
            10,
            16,
        )?;
        let tenant: TenantId = "tnt_bench".parse()?;
        let process: ProcessId = "prc_bench".parse()?;
        let mut expected = Vec::new();
        for index in (0..pending).rev() {
            let input = ProcessInput::new(
                tenant.clone(),
                process.clone(),
                format!("inp_{index:03}").parse()?,
                ProcessInputKind::CanonicalEvent,
                ContentDigest([2; 32]),
            );
            let receipt = block_on(store.accept(&input))??;
            receipt.validate_for(&input)?;
            if receipt.duplicate {
                return Err("unexpected duplicate".into());
            }
            expected.push(input);
        }
        expected.reverse();
        let limit = std::num::NonZeroU16::new(16).ok_or("zero limit")?;
        let call = || -> Result<(), Error> {
            let found = block_on(store.pending_inputs(&tenant, &process, limit))??;
            if found != expected {
                return Err("wrong discovery".into());
            }
            Ok(())
        };
        for _ in 0..64 {
            call()?;
        }
        measure(pending, &call)?;
        let connection = rusqlite::Connection::open(&path)?;
        let counts: (i64, i64) = connection.query_row(
            "SELECT count(*),count(commit_digest) FROM process_inbox",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if counts != (i64::from(pending), 0) {
            return Err("discovery consumed or created inputs".into());
        }
    }
    Ok(())
}
