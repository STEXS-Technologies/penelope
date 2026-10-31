//! Public replay latency at selected history sizes and page cursors.
//! Population uses typed public commits; validation is outside the timed region.
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
            return Err("replay timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
fn populate(
    store: &SqliteProcessStore,
    scope: &ProcessScope,
    count: u32,
) -> Result<Vec<ProcessOutcome>, Error> {
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        "inp_start".parse()?,
        ProcessInputKind::CanonicalEvent,
        ContentDigest([2; 32]),
    );
    let mut outcomes = Vec::new();
    for sequence in 0..count {
        outcomes.push(ProcessOutcome::new(
            scope.clone(),
            u64::from(sequence),
            ProcessOutcomeFact::new(
                format!("out_{sequence:06}").parse()?,
                CausationId::Input(input.input_id.clone()),
                OutcomeActor::System,
                LogicalTime(1),
                if sequence == 0 {
                    ProcessOutcomeKind::Started
                } else {
                    ProcessOutcomeKind::ActionPlanned
                },
                ContentDigest([3; 32]),
            ),
        ));
    }
    let mut start = 0_u64;
    for batch in outcomes.chunks(128) {
        let commit = AtomicProcessCommit::new(
            start,
            if start == 0 {
                Some(input.clone())
            } else {
                None
            },
            batch.to_vec(),
            Vec::new(),
        )?;
        block_on(ProcessStore::commit(store, &commit))??.validate_for(&commit)?;
        start = start
            .checked_add(u64::try_from(batch.len())?)
            .ok_or("sequence overflow")?;
    }
    Ok(outcomes)
}
fn verify(
    page: &OutcomeReplayPage,
    request: &OutcomeReplayRequest,
    history: &[ProcessOutcome],
) -> Result<(), Error> {
    page.validate()?;
    let expected: Vec<_> = history
        .iter()
        .filter(|outcome| outcome.sequence >= request.from_sequence)
        .take(usize::from(request.limit.get()))
        .cloned()
        .collect();
    let next = request
        .from_sequence
        .checked_add(u64::try_from(expected.len())?)
        .ok_or("sequence overflow")?;
    let continuation = if next < u64::try_from(history.len())? {
        Some(next)
    } else {
        None
    };
    if page.scope != request.scope
        || page.from_sequence != request.from_sequence
        || page.outcomes != expected
        || page.next_sequence != continuation
    {
        return Err("incorrect replay page".into());
    }
    Ok(())
}
fn main() -> Result<(), Error> {
    for count in [1_u32, 128, 4096] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("replay.db");
        let store =
            SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 10, 16)?;
        let scope = ProcessScope::new(
            "tnt_bench".parse()?,
            "prc_bench".parse()?,
            "def_bench".parse()?,
            "dfv_one".parse()?,
            ContentDigest([1; 32]),
        );
        let history = populate(&store, &scope, count)?;
        let last = count.checked_sub(1).ok_or("empty fixture")?;
        let past = count.checked_add(17).ok_or("cursor overflow")?;
        for (profile, from, limit) in [
            ("first", 0, 1),
            ("last", last, 1),
            ("end", count, 1),
            ("past", past, 1),
            ("page", 0, 128),
        ] {
            let request = OutcomeReplayRequest::new(
                scope.clone(),
                u64::from(from),
                NonZeroU16::new(limit).ok_or("zero page")?,
            )?;
            for _ in 0..100 {
                verify(
                    &block_on(store.read_outcomes(&request))??,
                    &request,
                    &history,
                )?;
            }
            let mut elapsed_ns = 0_u128;
            for _ in 0..1000 {
                let started = Instant::now();
                let page = block_on(store.read_outcomes(&request))??;
                elapsed_ns = elapsed_ns
                    .checked_add(started.elapsed().as_nanos())
                    .ok_or("duration overflow")?;
                verify(&page, &request, &history)?;
            }
            println!(
                "{}",
                serde_json::json!({"history":count,"profile":profile,"from":from,"limit":limit,"iterations":1000,"elapsed_ns":elapsed_ns})
            );
        }
        let connection = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let retained: u32 =
            connection.query_row("SELECT count(*) FROM process_outcomes", [], |row| {
                row.get(0)
            })?;
        if retained != count {
            return Err("history changed during replay".into());
        }
    }
    Ok(())
}
