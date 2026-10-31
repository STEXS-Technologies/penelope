//! Nonempty lease batches, including worker dispatch and WAL/FULL transactions.
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    num::NonZeroU16,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;
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
fn main() -> Result<(), Error> {
    for timers in [false, true] {
        for batch in [1_u16, 16] {
            let mut elapsed = Duration::ZERO;
            let mut total = 0_u32;
            for _ in 0..8 {
                let directory = tempfile::tempdir()?;
                let path = directory.path().join("batches.db");
                let clock = Arc::new(Clock(AtomicU64::new(100)));
                let store = SqliteProcessStore::open(
                    &path,
                    SqliteLimits::default(),
                    clock.clone(),
                    10,
                    16,
                )?;
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
                block_on(ProcessStore::commit(&store, &commit))??;
                if timers {
                    for action in &commit.actions {
                        block_on(store.schedule(&TimerSchedule {
                            action: action.clone(),
                            due_at: LogicalTime(1),
                        }))??;
                    }
                }
                let limit = NonZeroU16::new(batch).ok_or("zero batch")?;
                let outbox = OutboxClaimRequest::new(scope.clone(), "pri_bench".parse()?, limit)?;
                let mut tokens = Vec::new();
                for iteration in 0..60_u32 {
                    let now = 100_u64
                        .checked_add(
                            u64::from(iteration)
                                .checked_mul(11)
                                .ok_or("time overflow")?,
                        )
                        .ok_or("time overflow")?;
                    clock.0.store(now, Ordering::SeqCst);
                    let timer = TimerClaimRequest::new(
                        scope.clone(),
                        "pri_bench".parse()?,
                        LogicalTime(now),
                        limit,
                    )?;
                    let started = Instant::now();
                    let issued = if timers {
                        let leases = block_on(store.claim_due(&timer))??;
                        let duration = started.elapsed();
                        if iteration >= 10 {
                            elapsed = elapsed.checked_add(duration).ok_or("duration overflow")?;
                        }
                        leases
                            .into_iter()
                            .map(|lease| lease.token)
                            .collect::<Vec<_>>()
                    } else {
                        let leases = block_on(store.claim(&outbox))??;
                        let duration = started.elapsed();
                        if iteration >= 10 {
                            elapsed = elapsed.checked_add(duration).ok_or("duration overflow")?;
                        }
                        if leases
                            .iter()
                            .any(|lease| lease.record.delivery_attempt != iteration)
                        {
                            return Err("wrong attempt".into());
                        }
                        leases
                            .into_iter()
                            .map(|lease| lease.token)
                            .collect::<Vec<_>>()
                    };
                    if issued.len() != usize::from(batch) {
                        return Err("wrong batch size".into());
                    }
                    for token in issued {
                        if tokens.contains(&token) {
                            return Err("reused token".into());
                        }
                        tokens.push(token);
                    }
                    if iteration >= 10 {
                        total = total.checked_add(1).ok_or("count overflow")?;
                    }
                }
                let connection = rusqlite::Connection::open(&path)?;
                let next: i64 =
                    connection
                        .query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))?;
                let expected = 1_i64
                    .checked_add(i64::from(batch).checked_mul(60).ok_or("counter overflow")?)
                    .ok_or("counter overflow")?;
                if next != expected {
                    return Err("wrong persisted counter".into());
                }
            }
            println!(
                "timers={timers} batch={batch} iterations={total} elapsed_ns={}",
                elapsed.as_nanos()
            );
        }
    }
    Ok(())
}
