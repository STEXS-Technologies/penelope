//! Public claim fixture shared by timing and allocation probes.
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use serde::Serialize;
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
pub type Error = Box<dyn std::error::Error>;
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
fn wait<T>(future: impl Future<Output = T>) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or("deadline overflow")?;
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err("claim timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
#[derive(Serialize)]
pub enum Claimed {
    Outbox(Vec<OutboxLease>),
    Timer(Vec<TimerLease>),
}
pub struct Fixture {
    directory: tempfile::TempDir,
    store: SqliteProcessStore,
    clock: Arc<Clock>,
    commit: AtomicProcessCommit,
    request: OutboxClaimRequest,
    timer_request: TimerClaimRequest,
    timers: bool,
}
impl Fixture {
    pub fn new(timers: bool, length: usize, batch: u16) -> Result<Self, Error> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("claims.db");
        let clock = Arc::new(Clock(AtomicU64::new(100)));
        let store =
            SqliteProcessStore::open(&path, SqliteLimits::default(), clock.clone(), 10, 16)?;
        let suffix = "x".repeat(length.checked_sub(4).ok_or("short identifier")?);
        let scope = ProcessScope::new(
            format!("tnt_{suffix}").parse()?,
            format!("prc_{suffix}").parse()?,
            format!("def_{suffix}").parse()?,
            format!("dfv_{suffix}").parse()?,
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
        wait(ProcessStore::commit(&store, &commit))??;
        if timers {
            for action in &commit.actions {
                wait(store.schedule(&TimerSchedule {
                    action: action.clone(),
                    due_at: LogicalTime(1),
                }))??;
            }
        }
        let limit = NonZeroU16::new(batch).ok_or("empty batch")?;
        let request = OutboxClaimRequest::new(scope.clone(), "pri_bench".parse()?, limit)?;
        let timer_request =
            TimerClaimRequest::new(scope, "pri_bench".parse()?, LogicalTime(100), limit)?;
        Ok(Self {
            directory,
            store,
            clock,
            commit,
            request,
            timer_request,
            timers,
        })
    }
    pub fn prepare(&mut self, sample: u32) -> Result<(), Error> {
        let now = 100_u64
            .checked_add(u64::from(sample).checked_mul(11).ok_or("time overflow")?)
            .ok_or("time overflow")?;
        self.clock.0.store(now, Ordering::SeqCst);
        self.timer_request.now = LogicalTime(now);
        Ok(())
    }
    pub fn claim(&self) -> Result<Claimed, Error> {
        if self.timers {
            Ok(Claimed::Timer(wait(
                self.store.claim_due(&self.timer_request),
            )??))
        } else {
            Ok(Claimed::Outbox(wait(self.store.claim(&self.request))??))
        }
    }
    pub fn verify(&self, issued: &Claimed, sample: u32) -> Result<ContentDigest, Error> {
        let tokens: Vec<_> = match issued {
            Claimed::Timer(leases) => {
                if !self.timers || leases.len() != self.commit.actions.len() {
                    return Err("wrong timer batch".into());
                }
                for (lease, action) in leases.iter().zip(&self.commit.actions) {
                    lease.validate_for_claim(&self.timer_request)?;
                    if &lease.timer.action != action {
                        return Err("substituted timer action".into());
                    }
                }
                leases.iter().map(|lease| lease.token).collect()
            }
            Claimed::Outbox(leases) => {
                if self.timers || leases.len() != self.commit.actions.len() {
                    return Err("wrong outbox batch".into());
                }
                for (lease, action) in leases.iter().zip(&self.commit.actions) {
                    lease.validate_for_claim_at(&self.request, self.timer_request.now)?;
                    if &lease.record.action != action || lease.record.delivery_attempt != sample {
                        return Err("substituted outbox action or attempt".into());
                    }
                }
                leases.iter().map(|lease| lease.token).collect()
            }
        };
        let base = u64::from(sample)
            .checked_mul(u64::try_from(tokens.len())?)
            .ok_or("token overflow")?;
        for (index, token) in tokens.iter().enumerate() {
            let expected = base
                .checked_add(u64::try_from(index)?)
                .and_then(|v| v.checked_add(1))
                .ok_or("token overflow")?;
            if *token
                != OutboxLeaseToken::new(std::num::NonZeroU64::new(expected).ok_or("zero token")?)
            {
                return Err("wrong token".into());
            }
        }
        Ok(ContentDigest::sha256(&serde_json::to_vec(issued)?))
    }
    pub fn reopen(&self, issued: &Claimed, samples: u32) -> Result<(), Error> {
        let path = self.directory.path().join("claims.db");
        let connection = rusqlite::Connection::open(&path)?;
        let next: i64 =
            connection.query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))?;
        let expected = i64::from(samples)
            .checked_mul(i64::from(self.request.limit.get()))
            .and_then(|v| v.checked_add(1))
            .ok_or("counter overflow")?;
        if next != expected {
            return Err("wrong persisted counter".into());
        }
        let table = if self.timers {
            "process_timers"
        } else {
            "process_actions"
        };
        let mut statement = connection.prepare(&format!(
            "SELECT lease,attempt,expires_at FROM {table} ORDER BY action_id"
        ))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let expected_leases: Vec<_> = match issued {
            Claimed::Timer(leases) => leases
                .iter()
                .map(serde_json::to_vec)
                .collect::<Result<_, _>>()?,
            Claimed::Outbox(leases) => leases
                .iter()
                .map(serde_json::to_vec)
                .collect::<Result<_, _>>()?,
        };
        let persisted = rows.collect::<Result<Vec<_>, _>>()?;
        if persisted.len() != expected_leases.len() {
            return Err("wrong persisted batch".into());
        }
        let expires = self
            .timer_request
            .now
            .0
            .checked_add(10)
            .ok_or("expiry overflow")?;
        for ((bytes, attempt, expiry), expected_bytes) in persisted.into_iter().zip(expected_leases)
        {
            if bytes != expected_bytes || attempt != samples || expiry != i64::try_from(expires)? {
                return Err("wrong persisted envelope".into());
            }
        }
        let reopened =
            SqliteProcessStore::open(&path, SqliteLimits::default(), self.clock.clone(), 10, 16)?;
        let empty = if self.timers {
            wait(reopened.claim_due(&self.timer_request))??.is_empty()
        } else {
            wait(reopened.claim(&self.request))??.is_empty()
        };
        if !empty {
            return Err("unexpired lease reclaimed on reopen".into());
        }
        Ok(())
    }
}
