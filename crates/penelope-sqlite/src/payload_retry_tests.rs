//! Deterministic insertion race between the optimistic read and writer admission.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use super::*;
use crate::{SqliteLeaseClock, SqliteLimits};
use penelope_domain::LogicalTime;
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
struct Gate {
    entered: Sender<()>,
    resume: Receiver<()>,
}
static GATE: Mutex<Option<Gate>> = Mutex::new(None);
fn busy(attempt: i32) -> bool {
    if attempt != 0 {
        return false;
    }
    let guard = GATE.lock().unwrap();
    let gate = guard.as_ref().unwrap();
    gate.entered.send(()).is_ok() && gate.resume.recv_timeout(Duration::from_secs(5)).is_ok()
}
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "payload race operation timed out"
        );
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
fn snapshot(connection: &Connection) -> (i64, i64, Vec<u8>) {
    (
        connection
            .query_row("SELECT bytes_used FROM process_usage", [], |row| row.get(0))
            .unwrap(),
        connection
            .query_row("SELECT count(*) FROM process_payloads", [], |row| {
                row.get(0)
            })
            .unwrap(),
        connection
            .query_row(
                "SELECT bytes FROM process_payloads WHERE digest=?1",
                [ContentDigest::sha256(b"raced").0.as_slice()],
                |row| row.get(0),
            )
            .unwrap(),
    )
}
#[test]
fn insertion_after_read_is_rechecked_before_quota_charge() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("race.db");
    let scope = ProcessScope::new(
        "tnt_race".parse().unwrap(),
        "prc_race".parse().unwrap(),
        "def_race".parse().unwrap(),
        "dfv_one".parse().unwrap(),
        ContentDigest([3; 32]),
    );
    let seed = b"seed";
    let bytes = b"raced";
    let digest = ContentDigest::sha256(bytes);
    let charge = u64::try_from(scope.canonical_bytes().len() + bytes.len() + 128).unwrap();
    let quota = charge + u64::try_from(scope.canonical_bytes().len() + seed.len() + 128).unwrap();
    let adapter = SqliteProcessStore::open(
        &path,
        SqliteLimits {
            max_scope_bytes: quota,
            ..SqliteLimits::default()
        },
        Arc::new(Clock),
        10,
        16,
    )
    .unwrap();
    wait(adapter.put_payload(&scope, ContentDigest::sha256(seed), seed)).unwrap();
    wait(
        adapter
            .worker
            .run(|connection| {
                connection
                    .busy_handler(Some(busy))
                    .map_err(|error| db_error(&error))
            })
            .unwrap(),
    )
    .unwrap();
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    connection
        .execute(
            "INSERT INTO process_payloads VALUES (?1,?2,?3)",
            params![
                scope.canonical_bytes(),
                digest.0.as_slice(),
                bytes.as_slice()
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE process_usage SET bytes_used=bytes_used+?1",
            [i64::try_from(charge).unwrap()],
        )
        .unwrap();
    let before = snapshot(&connection);
    assert_eq!(before.0, i64::try_from(quota).unwrap());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    *GATE.lock().unwrap() = Some(Gate {
        entered: entered_tx,
        resume: resume_rx,
    });
    std::thread::scope(|threads| {
        let pending = threads.spawn(|| wait(adapter.put_payload(&scope, digest, bytes)));
        // The worker has read absence and reached BEGIN IMMEDIATE. No sleeps.
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        connection.execute_batch("COMMIT").unwrap();
        resume_tx.send(()).unwrap();
        assert_eq!(pending.join().unwrap(), Ok(digest));
    });
    *GATE.lock().unwrap() = None;
    assert_eq!(snapshot(&connection), before);
    assert_eq!(wait(adapter.read_payload(&scope, digest)).unwrap(), bytes);
    println!("payload_insert_race=true concurrent_insert_rechecked=true quota_charged_once=true");
}
