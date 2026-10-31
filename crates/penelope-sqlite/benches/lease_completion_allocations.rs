//! Rust allocation traffic for warm public lease completion; no latency claims.
#[path = "support/task_read_allocator.rs"]
mod allocator;
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
    for case in [
        "outbox_ack",
        "timer_ack",
        "outbox_renew",
        "timer_schedule",
        "timer_cancel",
    ] {
        let timers = case.starts_with("timer_");
        let batch = 1_u16;
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
        let mut outbox_lease = if timers {
            None
        } else {
            Some(block_on(store.claim(&outbox))??.remove(0))
        };
        let timer_lease = if timers {
            Some(
                block_on(store.claim_due(&TimerClaimRequest::new(
                    scope.clone(),
                    "pri_bench".parse()?,
                    LogicalTime(100),
                    limit,
                )?))??
                .remove(0),
            )
        } else {
            None
        };
        let schedule = TimerSchedule {
            action: commit.actions.first().ok_or("missing action")?.clone(),
            due_at: LogicalTime(1),
        };
        let mut call = || -> Result<(), Error> {
            if case == "timer_schedule" {
                block_on(store.schedule(&schedule))??;
            } else if case == "timer_cancel" {
                block_on(store.cancel(&schedule))??;
            } else if let Some(lease) = &timer_lease {
                block_on(TimerClaimStore::acknowledge(
                    &store,
                    lease,
                    LogicalTime(100),
                ))??;
            } else if let Some(lease) = &mut outbox_lease {
                if case == "outbox_renew" {
                    let next = lease
                        .lease_expires_at
                        .0
                        .checked_add(1)
                        .ok_or("expiry overflow")?;
                    *lease = block_on(store.renew(lease, LogicalTime(100), LogicalTime(next)))??;
                } else {
                    block_on(OutboxStore::acknowledge(&store, lease, LogicalTime(100)))??;
                }
            } else {
                return Err("missing lease".into());
            }
            Ok(())
        };
        for _ in 0..16 {
            call()?;
        }
        for sample in 0..32 {
            let (result, allocations, reallocations, requested_bytes) =
                allocator::measure(&mut call);
            result?;
            println!(
                "{}",
                serde_json::json!({"case":case,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes})
            );
        }
        let connection = rusqlite::Connection::open(&path)?;
        let terminal: i64 = connection.query_row(
            if timers {
                "SELECT state FROM process_timers"
            } else {
                "SELECT acknowledged FROM process_actions"
            },
            [],
            |row| row.get(0),
        )?;
        let expected = match case {
            "timer_schedule" | "outbox_renew" => 0,
            "timer_cancel" => 2,
            _ => 1,
        };
        if terminal != expected {
            return Err("wrong final state".into());
        }
        if case == "outbox_renew" {
            let expiry: i64 =
                connection.query_row("SELECT expires_at FROM process_actions", [], |row| {
                    row.get(0)
                })?;
            if expiry != 158 {
                return Err("wrong renewed expiry".into());
            }
        }
    }
    Ok(())
}
