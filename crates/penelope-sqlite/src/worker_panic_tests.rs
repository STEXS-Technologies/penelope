//! A caught panic may carry application code in its payload destructor.
#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    task::Wake,
    time::{Duration, Instant},
};
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
struct NestedPayload;
impl Drop for NestedPayload {
    fn drop(&mut self) {
        panic!("nested panic payload must not be destroyed");
    }
}
struct Payload(Arc<AtomicUsize>);
impl Drop for Payload {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        std::panic::panic_any(NestedPayload);
    }
}
struct PanicWake(Arc<AtomicUsize>);
impl Wake for PanicWake {
    fn wake(self: Arc<Self>) {
        std::panic::panic_any(Payload(self.0.clone()));
    }
}
fn wait<T>(response: Response<T>) -> Result<T, PortError> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut response = Box::pin(response);
    let deadline = Instant::now().checked_add(Duration::from_secs(5)).unwrap();
    loop {
        if let Poll::Ready(value) = response.as_mut().poll(&mut context) {
            return value;
        }
        if Instant::now() >= deadline {
            return Err(PortError::TimedOut);
        }
        std::thread::park_timeout(Duration::from_millis(5));
    }
}
fn scenario(panic_in_operation: bool, close_admission: bool, cancel_response: bool) {
    let initial_connection = Connection::open_in_memory().unwrap();
    initial_connection
        .execute_batch("CREATE TABLE writes(value INTEGER)")
        .unwrap();
    let worker = DatabaseWorker::new(initial_connection, 1).unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let operation_drops = drops.clone();
    let (entered, started) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let mut first = Box::pin(
        worker
            .run(move |connection| {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(5)).unwrap();
                connection
                    .execute("INSERT INTO writes VALUES (1)", [])
                    .map_err(|_database| PortError::Unavailable)?;
                if panic_in_operation {
                    std::panic::panic_any(Payload(operation_drops));
                }
                Ok(1_u32)
            })
            .unwrap(),
    );
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let waker = if panic_in_operation {
        Waker::from(Arc::new(ThreadWake(std::thread::current())))
    } else {
        Waker::from(Arc::new(PanicWake(drops.clone())))
    };
    assert!(
        first
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    let queued = worker
        .run(|connection| {
            connection
                .execute("INSERT INTO writes VALUES (2)", [])
                .map_err(|_database| PortError::Unavailable)?;
            connection
                .query_row("SELECT count(*) FROM writes", [], |row| {
                    row.get::<_, u32>(0)
                })
                .map_err(|_database| PortError::Unavailable)
        })
        .unwrap();
    let retained_worker = if close_admission {
        drop(worker);
        None
    } else {
        Some(worker)
    };
    let retained_response = if cancel_response {
        drop(first);
        None
    } else {
        Some(first)
    };
    release.send(()).unwrap();
    assert_eq!(
        wait(queued),
        Ok(2),
        "accepted work stranded by panic payload"
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    if let Some(response) = retained_response {
        assert_eq!(
            wait(*Pin::into_inner(response)),
            if panic_in_operation {
                Err(PortError::Unavailable)
            } else {
                Ok(1)
            }
        );
    }
    if let Some(open_worker) = retained_worker {
        assert_eq!(wait(open_worker.run(|_connection| Ok(3)).unwrap()), Ok(3));
    }
}
#[test]
fn operation_panic_payload_destructor_cannot_strand_accepted_work() {
    for close_admission in [false, true] {
        for cancel_response in [false, true] {
            scenario(true, close_admission, cancel_response);
        }
    }
}
#[test]
fn notification_panic_payload_destructor_cannot_strand_accepted_work() {
    for close_admission in [false, true] {
        for cancel_response in [false, true] {
            scenario(false, close_admission, cancel_response);
        }
    }
}

struct CanceledResult {
    drops: Arc<AtomicUsize>,
    nested: bool,
}
impl Drop for CanceledResult {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        if self.nested {
            std::panic::panic_any(NestedPayload);
        }
        panic!("injected canceled result destructor panic");
    }
}
fn canceled_result_scenarios(nested: bool) {
    for close_admission in [false, true] {
        for register_waker in [false, true] {
            let initial_connection = Connection::open_in_memory().unwrap();
            initial_connection
                .execute_batch("CREATE TABLE writes(value INTEGER)")
                .unwrap();
            let worker = DatabaseWorker::new(initial_connection, 1).unwrap();
            let drops = Arc::new(AtomicUsize::new(0));
            let result_drops = drops.clone();
            let (entered, started) = std::sync::mpsc::channel();
            let (release, resume) = std::sync::mpsc::channel();
            let mut canceled = Box::pin(
                worker
                    .run(move |connection| {
                        entered.send(()).unwrap();
                        resume.recv_timeout(Duration::from_secs(5)).unwrap();
                        connection
                            .execute("INSERT INTO writes VALUES (1)", [])
                            .map_err(|_database| PortError::Unavailable)?;
                        Ok(CanceledResult {
                            drops: result_drops,
                            nested,
                        })
                    })
                    .unwrap(),
            );
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            if register_waker {
                let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
                assert!(
                    canceled
                        .as_mut()
                        .poll(&mut Context::from_waker(&waker))
                        .is_pending()
                );
            }
            drop(canceled);
            let queued = worker
                .run(|connection| {
                    connection
                        .execute("INSERT INTO writes VALUES (2)", [])
                        .map_err(|_database| PortError::Unavailable)?;
                    connection
                        .query_row("SELECT count(*) FROM writes", [], |row| {
                            row.get::<_, u32>(0)
                        })
                        .map_err(|_database| PortError::Unavailable)
                })
                .unwrap();
            let retained = if close_admission {
                drop(worker);
                None
            } else {
                Some(worker)
            };
            release.send(()).unwrap();
            assert_eq!(wait(queued), Ok(2));
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            if let Some(live_worker) = retained {
                assert_eq!(wait(live_worker.run(|_connection| Ok(7)).unwrap()), Ok(7));
            }
            println!(
                "nested={nested} close_admission={close_admission} register_waker={register_waker} committed=2 dropped_once=true queued_completed=true"
            );
        }
    }
}

#[test]
fn canceled_result_destructor_cannot_strand_accepted_work() {
    canceled_result_scenarios(false);
}
#[test]
fn nested_result_destructor_cannot_abort_worker_or_strand_accepted_work() {
    canceled_result_scenarios(true);
}
