//! Public SDK fixture: retained acknowledged actions and future timers.
//! All population, acknowledgement and scheduling use typed public ports.
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
    num::NonZeroU16,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
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
fn fixture_commit(
    scope: &ProcessScope,
    batch: u32,
    timers: bool,
) -> Result<AtomicProcessCommit, Error> {
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        "inp_start".parse()?,
        ProcessInputKind::CanonicalEvent,
        ContentDigest([2; 32]),
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        u64::from(batch),
        ProcessOutcomeFact::new(
            format!("out_{batch:03}").parse()?,
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            if batch == 0 {
                ProcessOutcomeKind::Started
            } else {
                ProcessOutcomeKind::ActionPlanned
            },
            ContentDigest([3; 32]),
        ),
    );
    let mut actions = Vec::new();
    for offset in 0..128_u32 {
        let index = batch
            .checked_mul(128)
            .and_then(|start| start.checked_add(offset))
            .ok_or("action overflow")?;
        actions.push(ProcessAction::new(
            scope.clone(),
            format!("act_{index:06}").parse()?,
            format!("stp_{index:06}").parse()?,
            0,
            if timers {
                ProcessActionKind::Timer
            } else {
                ProcessActionKind::ExternalEffect
            },
            ContentDigest([4; 32]),
        ));
    }
    Ok(AtomicProcessCommit::new(
        u64::from(batch),
        if batch == 0 { Some(input) } else { None },
        vec![outcome],
        actions,
    )?)
}
fn main() -> Result<(), Error> {
    for timers in [false, true] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("history.db");
        let open_started = Instant::now();
        let store =
            SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 10, 16)?;
        let open_ns = open_started.elapsed().as_nanos();
        let scope = ProcessScope::new(
            "tnt_bench".parse()?,
            "prc_bench".parse()?,
            "def_bench".parse()?,
            "dfv_one".parse()?,
            ContentDigest([1; 32]),
        );
        let mut commit_ns = 0_u128;
        let mut schedule_ns = 0_u128;
        for batch in 0..8 {
            let commit = fixture_commit(&scope, batch, timers)?;
            let commit_started = Instant::now();
            let receipt = block_on(ProcessStore::commit(&store, &commit))??;
            commit_ns = commit_ns
                .checked_add(commit_started.elapsed().as_nanos())
                .ok_or("duration overflow")?;
            receipt.validate_for(&commit)?;
            if timers {
                for action in commit.actions {
                    let timer = TimerSchedule {
                        action,
                        due_at: LogicalTime(200),
                    };
                    let schedule_started = Instant::now();
                    block_on(store.schedule(&timer))??;
                    schedule_ns = schedule_ns
                        .checked_add(schedule_started.elapsed().as_nanos())
                        .ok_or("duration overflow")?;
                }
            }
        }
        let mut claim_ns = 0_u128;
        let mut ack_ns = 0_u128;
        if !timers {
            let request = OutboxClaimRequest::new(
                scope.clone(),
                "pri_bench".parse()?,
                NonZeroU16::new(128).ok_or("zero batch")?,
            )?;
            for batch in 0..8_u32 {
                let claim_started = Instant::now();
                let leases = block_on(store.claim(&request))??;
                claim_ns = claim_ns
                    .checked_add(claim_started.elapsed().as_nanos())
                    .ok_or("duration overflow")?;
                if leases.len() != 128 {
                    return Err("wrong retained batch".into());
                }
                for (offset, lease) in leases.into_iter().enumerate() {
                    let index = batch
                        .checked_mul(128)
                        .and_then(|start| start.checked_add(u32::try_from(offset).ok()?))
                        .ok_or("action overflow")?;
                    if lease.record.action.action_id.as_str() != format!("act_{index:06}") {
                        return Err("wrong retained order".into());
                    }
                    let ack_started = Instant::now();
                    block_on(OutboxStore::acknowledge(&store, &lease, LogicalTime(100)))??;
                    ack_ns = ack_ns
                        .checked_add(ack_started.elapsed().as_nanos())
                        .ok_or("duration overflow")?;
                }
            }
        }
        let limit = NonZeroU16::new(16).ok_or("zero batch")?;
        let outbox = OutboxClaimRequest::new(scope.clone(), "pri_bench".parse()?, limit)?;
        let timer = TimerClaimRequest::new(scope, "pri_bench".parse()?, LogicalTime(100), limit)?;
        for _ in 0..100 {
            if timers {
                if !block_on(store.claim_due(&timer))??.is_empty() {
                    return Err("future timer claimed".into());
                }
            } else if !block_on(store.claim(&outbox))??.is_empty() {
                return Err("acknowledged action claimed".into());
            }
        }
        let idle_started = Instant::now();
        for _ in 0..1_000 {
            if timers {
                if !block_on(store.claim_due(&timer))??.is_empty() {
                    return Err("future timer claimed".into());
                }
            } else if !block_on(store.claim(&outbox))??.is_empty() {
                return Err("acknowledged action claimed".into());
            }
        }
        let idle_ns = idle_started.elapsed().as_nanos();
        let connection = rusqlite::Connection::open(&path)?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let count:u32=connection.query_row(if timers {"SELECT count(*) FROM process_timers WHERE state=0 AND due_at=200 AND attempt=0 AND lease IS NULL"} else {"SELECT count(*) FROM process_actions WHERE acknowledged=1 AND attempt=1 AND lease IS NOT NULL"},[],|row|row.get(0))?;
        let next: i64 =
            connection.query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))?;
        if count != 1024 || next != if timers { 1 } else { 1025 } {
            return Err("wrong retained state or counter".into());
        }
        println!(
            "{}",
            serde_json::json!({"timers":timers,"retained_rows":count,"schema_version":version,"open_ns":open_ns,"commit_ns":commit_ns,"commit_calls":8,"schedule_ns":schedule_ns,"schedule_calls":if timers {1024} else {0},"seed_claim_ns":claim_ns,"seed_claim_calls":if timers {0} else {8},"ack_ns":ack_ns,"ack_calls":if timers {0} else {1024},"idle_ns":idle_ns,"idle_calls":1000,"warmup_calls":100,"next_token":next})
        );
    }
    Ok(())
}
