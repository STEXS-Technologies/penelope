//! Public pending-inbox reads and admission/consumption with retained history.
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use std::{
    future::Future,
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
            return Err("inbox operation timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
use penelope_ports::inbox::PendingInbox;
use rusqlite::{Connection, StatementStatus, params};
use std::num::NonZeroU16;
const QUERY: &str = "SELECT input_id,input,checksum FROM process_inbox WHERE process_key=?1 AND commit_digest IS NULL ORDER BY input_id LIMIT ?2";
fn input(scope: &ProcessScope, name: &str) -> Result<ProcessInput, Error> {
    Ok(ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        name.parse()?,
        ProcessInputKind::CanonicalEvent,
        ContentDigest([2; 32]),
    ))
}
fn consume(
    store: &SqliteProcessStore,
    scope: &ProcessScope,
    input: ProcessInput,
    sequence: u64,
) -> Result<u128, Error> {
    let outcome = ProcessOutcome::new(
        scope.clone(),
        sequence,
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
    );
    let commit = AtomicProcessCommit::new(sequence, Some(input), vec![outcome], Vec::new())?;
    let started = Instant::now();
    let receipt = block_on(ProcessStore::commit(store, &commit))??;
    let elapsed = started.elapsed().as_nanos();
    receipt.validate_for(&commit)?;
    if !receipt.duplicate_input {
        return Err("accepted input not recognized".into());
    }
    Ok(elapsed)
}
fn discover(
    store: &SqliteProcessStore,
    scope: &ProcessScope,
    expected: &[ProcessInput],
) -> Result<u128, Error> {
    let limit = NonZeroU16::new(16).ok_or("zero limit")?;
    for _ in 0..100 {
        let found = block_on(store.pending_inputs(&scope.tenant_id, &scope.process_id, limit))??;
        if found != expected {
            return Err("incorrect warm discovery".into());
        }
    }
    let mut elapsed = 0_u128;
    for _ in 0..1000 {
        let started = Instant::now();
        let found = block_on(store.pending_inputs(&scope.tenant_id, &scope.process_id, limit))??;
        elapsed = elapsed
            .checked_add(started.elapsed().as_nanos())
            .ok_or("duration overflow")?;
        if found != expected {
            return Err("incorrect discovery".into());
        }
    }
    Ok(elapsed)
}
fn engine(
    connection: &Connection,
    scope: &ProcessScope,
    expected: &[ProcessInput],
) -> Result<serde_json::Value, Error> {
    let key = serde_json::to_vec(&(&scope.tenant_id, &scope.process_id))?;
    let plan = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {QUERY}"))?
        .query_map(params![key, 16], |row| row.get::<_, String>(3))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut statement = connection.prepare(QUERY)?;
    let selected = statement
        .query_map(params![key, 16], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if selected
        != expected
            .iter()
            .map(|input| input.input_id.as_str())
            .collect::<Vec<_>>()
    {
        return Err("incorrect engine discovery".into());
    }
    Ok(
        serde_json::json!({"vm_steps":statement.get_status(StatementStatus::VmStep),"sorts":statement.get_status(StatementStatus::Sort),"plan":plan,"identities":selected}),
    )
}
fn main() -> Result<(), Error> {
    for retained in [0_u32, 128, 1024] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("inbox.db");
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
        let mut seed_accept_ns = 0_u128;
        let mut seed_commit_ns = 0_u128;
        for index in 0..retained {
            let next = input(&scope, &format!("inp_done_{index:06}"))?;
            let started = Instant::now();
            let receipt = block_on(store.accept(&next))??;
            seed_accept_ns = seed_accept_ns
                .checked_add(started.elapsed().as_nanos())
                .ok_or("duration overflow")?;
            receipt.validate_for(&next)?;
            seed_commit_ns = seed_commit_ns
                .checked_add(consume(&store, &scope, next, u64::from(index))?)
                .ok_or("duration overflow")?;
        }
        let connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let empty_ns = discover(&store, &scope, &[])?;
        let empty_engine = engine(&connection, &scope, &[])?;
        let mut pending = Vec::new();
        let mut pending_accept_ns = 0_u128;
        for index in (0..16).rev() {
            let next = input(&scope, &format!("inp_pending_{index:03}"))?;
            let started = Instant::now();
            let receipt = block_on(store.accept(&next))??;
            pending_accept_ns = pending_accept_ns
                .checked_add(started.elapsed().as_nanos())
                .ok_or("duration overflow")?;
            receipt.validate_for(&next)?;
            pending.push(next);
        }
        pending.reverse();
        let pending_ns = discover(&store, &scope, &pending)?;
        let pending_engine = engine(&connection, &scope, &pending)?;
        let mut pending_commit_ns = 0_u128;
        for (offset, next) in pending.into_iter().enumerate() {
            let sequence = u64::from(retained)
                .checked_add(u64::try_from(offset)?)
                .ok_or("sequence overflow")?;
            pending_commit_ns = pending_commit_ns
                .checked_add(consume(&store, &scope, next, sequence)?)
                .ok_or("duration overflow")?;
        }
        if !block_on(store.pending_inputs(
            &scope.tenant_id,
            &scope.process_id,
            NonZeroU16::new(16).ok_or("zero limit")?,
        ))??
        .is_empty()
        {
            return Err("consumed work remains pending".into());
        }
        let consumed: u32 = connection.query_row(
            "SELECT count(*) FROM process_inbox WHERE commit_digest IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        if consumed != retained.checked_add(16).ok_or("count overflow")? {
            return Err("incorrect retained count".into());
        }
        let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let pages: u32 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        println!(
            "{}",
            serde_json::json!({"retained":retained,"schema_version":version,"sqlite":rusqlite::version(),"open_ns":open_ns,"seed_accept_ns":seed_accept_ns,"seed_commit_ns":seed_commit_ns,"empty_ns":empty_ns,"pending_ns":pending_ns,"pending_accept_ns":pending_accept_ns,"pending_commit_ns":pending_commit_ns,"read_calls":1000,"pending_calls":16,"consumed_rows":consumed,"pages":pages,"empty_engine":empty_engine,"pending_engine":pending_engine})
        );
    }
    Ok(())
}
