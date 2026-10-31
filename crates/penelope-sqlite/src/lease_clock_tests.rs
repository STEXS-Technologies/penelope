//! Real lock contention with a deterministic logical-clock advance.
use super::*;
use std::{
    cell::RefCell,
    future::Future,
    num::NonZeroU16,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
};
struct Clock {
    now: AtomicU64,
    calls: AtomicUsize,
    fail: AtomicBool,
}
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(PortError::Unavailable);
        }
        Ok(LogicalTime(self.now.load(Ordering::SeqCst)))
    }
}
struct Busy {
    entered: Option<mpsc::Sender<()>>,
    resume: mpsc::Receiver<()>,
}
thread_local! { static BUSY: RefCell<Option<Busy>> = const { RefCell::new(None) }; }
// SQLite invokes this on the worker during a real writer-lock conflict. It must
// return normally even if the test controller has failed or dropped its channels.
fn wait_for_release(_attempt: i32) -> bool {
    BUSY.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return false;
        };
        let Some(busy) = slot.as_mut() else {
            return false;
        };
        if let Some(entered) = busy.entered.take() {
            if entered.send(()).is_err() {
                return false;
            }
        }
        busy.resume.recv_timeout(Duration::from_secs(5)).is_ok()
    })
}
fn wait<T>(future: impl Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    let deadline = Instant::now().checked_add(Duration::from_secs(10)).unwrap();
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return result;
        }
        assert!(Instant::now() < deadline, "lease test future timed out");
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
fn contended_claim(reclaim: bool) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("lease-clock.db");
    let clock = Arc::new(Clock {
        now: AtomicU64::new(10),
        calls: AtomicUsize::new(0),
        fail: AtomicBool::new(false),
    });
    let store =
        SqliteProcessStore::open(&path, SqliteLimits::default(), clock.clone(), 10, 2).unwrap();
    wait(ProcessStore::commit(&store, &commit())).unwrap();
    let request =
        OutboxClaimRequest::new(scope(), id("pri_clock"), NonZeroU16::new(1).unwrap()).unwrap();
    let previous = if reclaim {
        let claimed = wait(OutboxStore::claim(&store, &request)).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed.first().unwrap().lease_expires_at, LogicalTime(20));
        Some(claimed.first().unwrap().clone())
    } else {
        None
    };
    let initial_calls = clock.calls.load(Ordering::SeqCst);
    let (entered, blocked) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    wait(
        store
            .worker
            .run(move |connection| {
                BUSY.with(|slot| {
                    *slot.borrow_mut() = Some(Busy {
                        entered: Some(entered),
                        resume,
                    })
                });
                connection
                    .busy_handler(Some(wait_for_release))
                    .map_err(|error| db_error(&error))
            })
            .unwrap(),
    )
    .unwrap();
    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut pending = Box::pin(OutboxStore::claim(&store, &request));
    assert!(
        pending
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
    // The production claim is definitely waiting inside SQLite. No elapsed-time
    // assumption is needed to advance time past the old lease window.
    let sampled_while_blocked = clock
        .calls
        .load(Ordering::SeqCst)
        .saturating_sub(initial_calls);
    clock.now.store(100, Ordering::SeqCst);
    holder.execute_batch("ROLLBACK").unwrap();
    release.send(()).unwrap();
    let claimed = wait(pending).unwrap();
    // Remove the test handler before any follow-up operations.
    wait(
        store
            .worker
            .run(|connection| {
                connection
                    .busy_handler(None)
                    .map_err(|error| db_error(&error))?;
                BUSY.with(|slot| *slot.borrow_mut() = None);
                Ok(())
            })
            .unwrap(),
    )
    .unwrap();
    println!(
        "reclaim={reclaim} clock_samples_while_blocked={sampled_while_blocked} returned_leases={} expiries={:?}",
        claimed.len(),
        claimed
            .iter()
            .map(|lease| lease.lease_expires_at)
            .collect::<Vec<_>>()
    );
    assert_eq!(claimed.len(), 1);
    let lease = claimed.first().unwrap();
    assert_eq!(lease.lease_expires_at, LogicalTime(110));
    lease.validate_at(LogicalTime(100)).unwrap();
    assert_eq!(
        sampled_while_blocked, 0,
        "clock sampled before writer lock acquisition"
    );

    if let Some(previous) = previous {
        assert_ne!(previous.token, lease.token);
    }
    assert!(
        wait(OutboxStore::claim(&store, &request))
            .unwrap()
            .is_empty()
    );
    wait(OutboxStore::acknowledge(&store, lease, LogicalTime(100))).unwrap();
    let reopened = SqliteProcessStore::open(&path, SqliteLimits::default(), clock, 10, 2).unwrap();
    assert!(
        wait(OutboxStore::claim(&reopened, &request))
            .unwrap()
            .is_empty()
    );
}
#[test]
fn new_outbox_claim_samples_clock_after_contended_writer_lock() {
    contended_claim(false);
}
#[test]
fn expired_outbox_lease_is_reclaimed_using_time_after_writer_lock() {
    contended_claim(true);
}

#[test]
fn clock_failure_and_expiry_overflow_release_the_writer_lock() {
    for fail_clock in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("clock-rollback.db");
        let clock = Arc::new(Clock {
            now: AtomicU64::new(if fail_clock { 10 } else { u64::MAX }),
            calls: AtomicUsize::new(0),
            fail: AtomicBool::new(fail_clock),
        });
        let store =
            SqliteProcessStore::open(&path, SqliteLimits::default(), clock.clone(), 10, 2).unwrap();
        wait(ProcessStore::commit(&store, &commit())).unwrap();
        let request =
            OutboxClaimRequest::new(scope(), id("pri_clock"), NonZeroU16::new(1).unwrap()).unwrap();
        let expected = if fail_clock {
            PortError::Unavailable
        } else {
            PortError::QuotaExceeded
        };
        assert_eq!(wait(OutboxStore::claim(&store, &request)), Err(expected));
        let connection = Connection::open(&path).unwrap();
        connection.busy_timeout(Duration::ZERO).unwrap();
        connection
            .execute_batch("BEGIN IMMEDIATE; ROLLBACK")
            .unwrap();
        let next_token: i64 = connection
            .query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))
            .unwrap();
        assert_eq!(next_token, 1);
        clock.now.store(10, Ordering::SeqCst);
        let leases = wait(OutboxStore::claim(&store, &request)).unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases.first().unwrap().lease_expires_at, LogicalTime(20));
    }
}
