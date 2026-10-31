//! Deterministic insertion race between the optimistic read and writer admission.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use super::*;
use crate::{SqliteLeaseClock, SqliteLimits};
use penelope_domain::{ContentDigest, LogicalTime, ProcessInputKind};
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
        assert!(Instant::now() < deadline, "inbox race operation timed out");
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
fn input(name: &str) -> ProcessInput {
    ProcessInput::new(
        "tnt_race".parse().unwrap(),
        "prc_race".parse().unwrap(),
        name.parse().unwrap(),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([3; 32]),
    )
}
fn snapshot(connection: &Connection) -> (i64, i64, Vec<u8>, Vec<u8>, Option<i64>) {
    let (bytes, checksum, consumed) = connection
        .query_row(
            "SELECT input,checksum,committed_through FROM process_inbox WHERE input_id='inp_raced'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    (
        connection
            .query_row("SELECT bytes_used FROM process_usage", [], |row| row.get(0))
            .unwrap(),
        connection
            .query_row("SELECT count(*) FROM process_inbox", [], |row| row.get(0))
            .unwrap(),
        bytes,
        checksum,
        consumed,
    )
}
#[test]
fn concurrent_admission_is_rechecked_before_quota_charge() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inbox-race.db");
    let seed = input("inp_seed");
    let wanted = input("inp_raced");
    let bytes = serde_json::to_vec(&wanted).unwrap();
    let charge = u64::try_from(bytes.len() + 128).unwrap();
    let quota = charge + u64::try_from(serde_json::to_vec(&seed).unwrap().len() + 128).unwrap();
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
    assert!(!wait(adapter.accept(&seed)).unwrap().duplicate);
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
            "INSERT INTO process_inbox(process_key,input_id,input,checksum) VALUES (?1,?2,?3,?4)",
            params![
                process_key(&wanted).unwrap(),
                wanted.input_id.as_str(),
                bytes,
                Digest::of(&bytes).0.as_slice()
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
        let pending = threads.spawn(|| wait(adapter.accept(&wanted)));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        connection.execute_batch("COMMIT").unwrap();
        resume_tx.send(()).unwrap();
        let receipt = pending.join().unwrap().unwrap();
        receipt.validate_for(&wanted).unwrap();
        assert!(receipt.duplicate);
    });
    *GATE.lock().unwrap() = None;
    assert_eq!(snapshot(&connection), before);
    assert!(before.4.is_none());
    println!(
        "inbox_insert_race=true concurrent_admission_rechecked=true quota_charged_once=true unconsumed=true"
    );
}
