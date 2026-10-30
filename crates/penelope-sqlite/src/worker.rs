//! Bounded blocking isolation for async port implementations, independent of
//! Tokio or another application executor. Cancellation can lose an ACK after
//! commit; callers must reconstruct authoritative records before retrying.
use penelope_ports::PortError;
use rusqlite::Connection;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        mpsc::{SyncSender, sync_channel},
    },
    task::{Context, Poll, Waker},
};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;
struct Completion<T> {
    result: Option<Result<T, PortError>>,
    waker: Option<Waker>,
}
pub(crate) struct Response<T> {
    completion: Arc<Mutex<Completion<T>>>,
}
impl<T> Future for Response<T> {
    type Output = Result<T, PortError>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Ok(mut state) = self.completion.lock() else {
            return Poll::Ready(Err(PortError::Unavailable));
        };
        state.result.take().map_or_else(
            || {
                state.waker = Some(context.waker().clone());
                Poll::Pending
            },
            Poll::Ready,
        )
    }
}

pub(crate) struct DatabaseWorker {
    sender: SyncSender<Job>,
    capacity: u16,
}
impl DatabaseWorker {
    pub(crate) const fn capacity(&self) -> u16 {
        self.capacity
    }
    pub(crate) fn new(mut connection: Connection, capacity: u16) -> Result<Self, PortError> {
        if capacity == 0 || capacity > 1024 {
            return Err(PortError::QuotaExceeded);
        }
        let (sender, receiver) = sync_channel::<Job>(usize::from(capacity));
        std::thread::Builder::new()
            .name("penelope-sqlite".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    job(&mut connection);
                }
            })
            .map_err(|_thread| PortError::Unavailable)?;
        Ok(Self { sender, capacity })
    }

    pub(crate) fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, PortError> + Send + 'static,
    ) -> Result<Response<T>, PortError> {
        let completion = Arc::new(Mutex::new(Completion {
            result: None,
            waker: None,
        }));
        let result_state = completion.clone();
        self.sender
            .try_send(Box::new(move |connection| {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    operation(connection)
                }))
                .unwrap_or(Err(PortError::Unavailable));
                let waker = result_state.lock().map_or_else(
                    |_poison| None,
                    |mut state| {
                        state.result = Some(result);
                        state.waker.take()
                    },
                );
                if let Some(waker) = waker {
                    // Executor callbacks are application code. The result is already
                    // published and unlocked; an unwinding notification must not
                    // kill the worker and strand unrelated accepted requests.
                    let _wake_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            waker.wake();
                        }));
                }
            }))
            .map_err(|error| match error {
                std::sync::mpsc::TrySendError::Full(_) => PortError::QuotaExceeded,
                std::sync::mpsc::TrySendError::Disconnected(_) => PortError::Unavailable,
            })?;
        Ok(Response { completion })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
    use super::*;
    use std::{
        task::Wake,
        time::{Duration, Instant},
    };
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    fn wait<T>(response: Response<T>) -> Result<T, PortError> {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut response = Box::pin(response);
        let deadline = Instant::now() + Duration::from_secs(5);
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
    #[test]
    fn queue_exhaustion_is_immediate_and_cancellation_does_not_undo_commits() {
        let initial_connection = Connection::open_in_memory().unwrap();
        initial_connection
            .execute_batch("CREATE TABLE writes(value INTEGER)")
            .unwrap();
        let worker = DatabaseWorker::new(initial_connection, 1).unwrap();
        let (entered, started) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let canceled = worker
            .run(move |connection| {
                entered.send(()).unwrap();
                resume.recv().unwrap();
                connection
                    .execute("INSERT INTO writes VALUES (1)", [])
                    .map_err(|_database| PortError::Unavailable)?;
                Ok(())
            })
            .unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(canceled);
        let pending = worker
            .run(|connection| {
                connection
                    .query_row("SELECT count(*) FROM writes", [], |row| {
                        row.get::<_, u32>(0)
                    })
                    .map_err(|_database| PortError::Unavailable)
            })
            .unwrap();
        assert!(matches!(
            worker.run(|_connection| Ok(())),
            Err(PortError::QuotaExceeded)
        ));
        release.send(()).unwrap();
        assert_eq!(wait(pending).unwrap(), 1);
        assert_eq!(
            wait(
                worker
                    .run(|_connection| -> Result<(), PortError> {
                        panic!("injected worker operation panic")
                    })
                    .unwrap()
            ),
            Err(PortError::Unavailable)
        );
        assert_eq!(wait(worker.run(|_connection| Ok(7)).unwrap()).unwrap(), 7);
    }
    #[test]
    fn panicking_waker_cannot_strand_accepted_work_or_discard_committed_result() {
        struct PanicWake;
        impl Wake for PanicWake {
            fn wake(self: Arc<Self>) {
                panic!("injected executor wake panic");
            }
        }
        for (close_admission, cancel_first) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let initial_connection = Connection::open_in_memory().unwrap();
            initial_connection
                .execute_batch("CREATE TABLE writes(value INTEGER)")
                .unwrap();
            let worker = DatabaseWorker::new(initial_connection, 1).unwrap();
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
                        Ok(1)
                    })
                    .unwrap(),
            );
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            let waker = Waker::from(Arc::new(PanicWake));
            let mut context = Context::from_waker(&waker);
            assert!(first.as_mut().poll(&mut context).is_pending());
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
            let open_worker = if close_admission {
                drop(worker);
                None
            } else {
                Some(worker)
            };
            // A dropped future can retain its registered executor waker until
            // the already accepted operation publishes its result.
            let retained_response = if cancel_first {
                drop(first);
                None
            } else {
                Some(first)
            };
            release.send(()).unwrap();
            assert_eq!(wait(queued), Ok(2));
            if let Some(response) = retained_response {
                assert_eq!(wait(*Pin::into_inner(response)), Ok(1));
            }
            if let Some(remaining_worker) = open_worker {
                assert_eq!(
                    wait(remaining_worker.run(|_connection| Ok(3)).unwrap()),
                    Ok(3)
                );
            }
        }
    }
}
