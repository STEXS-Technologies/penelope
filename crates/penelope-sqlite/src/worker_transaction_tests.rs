//! Transaction rollback and a lost ACK must leave different durable histories.
#![allow(clippy::unwrap_used, clippy::panic)]
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
fn values(connection: &Connection) -> Vec<i64> {
    connection
        .prepare("SELECT value FROM writes ORDER BY value")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}
#[test]
fn transaction_panics_preserve_commit_boundary_and_drain_accepted_work() {
    for commit_before_panic in [false, true] {
        for cancel_response in [false, true] {
            for close_admission in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("transaction-panic.db");
                let initial_connection = Connection::open(&path).unwrap();
                initial_connection
                    .execute_batch(
                        "PRAGMA journal_mode=WAL; CREATE TABLE writes(value INTEGER PRIMARY KEY)",
                    )
                    .unwrap();
                let worker = DatabaseWorker::new(initial_connection, 1).unwrap();
                let (entered, started) = std::sync::mpsc::channel();
                let (release, resume) = std::sync::mpsc::channel();
                let response = worker
                    .run(move |connection| -> Result<(), PortError> {
                        let transaction = connection.transaction().unwrap();
                        transaction
                            .execute("INSERT INTO writes VALUES (1)", [])
                            .unwrap();
                        entered.send(()).unwrap();
                        resume.recv_timeout(Duration::from_secs(5)).unwrap();
                        if commit_before_panic {
                            transaction.commit().unwrap();
                        }
                        panic!("injected panic at commit boundary");
                    })
                    .unwrap();
                started.recv_timeout(Duration::from_secs(5)).unwrap();
                let response = if cancel_response {
                    drop(response);
                    None
                } else {
                    Some(response)
                };
                let queued = worker
                    .run(|connection| {
                        // A rollback failure would leave the connection inside the
                        // previous transaction and prevent this fresh transaction.
                        let transaction = connection.transaction().unwrap();
                        transaction
                            .execute("INSERT INTO writes VALUES (2)", [])
                            .unwrap();
                        transaction.commit().unwrap();
                        Ok(values(connection))
                    })
                    .unwrap();
                let retained_worker = if close_admission {
                    drop(worker);
                    None
                } else {
                    Some(worker)
                };
                release.send(()).unwrap();
                let expected = if commit_before_panic {
                    vec![1, 2]
                } else {
                    vec![2]
                };
                assert_eq!(wait(queued), Ok(expected.clone()));
                if let Some(response) = response {
                    assert_eq!(wait(response), Err(PortError::Unavailable));
                }
                let reopened = Connection::open(&path).unwrap();
                assert_eq!(values(&reopened), expected);
                let integrity: String = reopened
                    .query_row("PRAGMA integrity_check", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(integrity, "ok");
                if let Some(remaining_worker) = retained_worker {
                    assert_eq!(
                        wait(
                            remaining_worker
                                .run(|connection| Ok(values(connection)))
                                .unwrap()
                        ),
                        Ok(expected)
                    );
                }
                println!(
                    "committed={commit_before_panic} canceled={cancel_response} closed={close_admission}: durable history and queue verified"
                );
            }
        }
    }
}
