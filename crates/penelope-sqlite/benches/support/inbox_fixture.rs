//! Rust allocation traffic for duplicate input admission; no timing claims.
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
    mut measure: impl FnMut(bool, &dyn Fn() -> Result<(), Error>) -> Result<(), Error>,
) -> Result<(), Error> {
    for consumed in [false, true] {
        let batch = 1_u16;
        let timers = false;
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("batches.db");
        let clock = Arc::new(Clock(AtomicU64::new(100)));
        let store =
            SqliteProcessStore::open(&path, SqliteLimits::default(), clock.clone(), 10, 16)?;
        let scope = ProcessScope::new(
            "tnt_bench".parse()?,
            "prc_bench".parse()?,
            "def_bench".parse()?,
            "dfv_one".parse()?,
            ContentDigest([1; 32]),
        );
        let input = ProcessInput::new(
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            "inp_start".parse()?,
            ProcessInputKind::CanonicalEvent,
            ContentDigest([2; 32]),
        );
        let outcome = ProcessOutcome::new(
            scope.clone(),
            0,
            ProcessOutcomeFact::new(
                "out_start".parse()?,
                CausationId::Input(input.input_id.clone()),
                OutcomeActor::System,
                LogicalTime(1),
                ProcessOutcomeKind::Started,
                ContentDigest([3; 32]),
            ),
        );
        let mut actions = Vec::new();
        for index in 0..batch {
            actions.push(ProcessAction::new(
                scope.clone(),
                format!("act_{index:03}").parse()?,
                format!("stp_{index:03}").parse()?,
                0,
                if timers {
                    ProcessActionKind::Timer
                } else {
                    ProcessActionKind::ExternalEffect
                },
                ContentDigest([4; 32]),
            ));
        }
        let commit = AtomicProcessCommit::new(0, Some(input), vec![outcome], actions)?;
        let accepted_input = commit.input.as_ref().ok_or("missing input")?;
        if consumed {
            block_on(ProcessStore::commit(&store, &commit))??;
        } else {
            block_on(store.accept(accepted_input))??;
        }
        let call = || -> Result<(), Error> {
            let receipt = block_on(store.accept(accepted_input))??;
            if !receipt.duplicate {
                return Err("not a duplicate receipt".into());
            }
            Ok(())
        };
        for _ in 0..64 {
            call()?;
        }
        measure(consumed, &call)?;
        let connection = rusqlite::Connection::open(&path)?;
        let counts: (i64,i64,i64,i64) = connection.query_row("SELECT (SELECT count(*) FROM process_inbox),(SELECT count(*) FROM process_inbox WHERE commit_digest IS NOT NULL),(SELECT count(*) FROM process_commits),(SELECT count(*) FROM process_outcomes)",[],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)))?;
        let expected = i64::from(consumed);
        if counts != (1, expected, expected, expected) {
            return Err("duplicate changed input state".into());
        }
    }
    Ok(())
}
