//! Deterministic SQLite workloads; run the binary with an external RSS profiler.
//! A reopened connection is not an OS-page-cache-cold measurement.
#![allow(clippy::arithmetic_side_effects)]
use penelope_core::task::{runtime::TaskJournal, *};
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore, SqliteTaskJournal};
use serde_json::{Value, json};
use std::{
    future::Future,
    hint::black_box,
    num::NonZeroU16,
    path::Path,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;

fn distribution(mut samples: Vec<u128>) -> Result<Value, Error> {
    samples.sort_unstable();
    let at = |percent: usize| -> Result<u128, Error> {
        let rank = samples
            .len()
            .checked_mul(percent)
            .ok_or("percentile overflow")?
            .div_ceil(100);
        samples
            .get(rank.saturating_sub(1))
            .copied()
            .ok_or_else(|| "empty samples".into())
    };
    Ok(json!({"samples": samples.len(), "p50_ns": at(50)?, "p95_ns": at(95)?, "p99_ns": at(99)?}))
}
fn timed<T>(operation: impl FnOnce() -> Result<T, Error>) -> Result<(T, u128), Error> {
    let start = Instant::now();
    let value = black_box(operation()?);
    Ok((value, start.elapsed().as_nanos()))
}
const fn definition() -> TaskDefinition {
    TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromCheckpoint,
            max_attempts: 3,
            timeout_ticks: 4096,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 64,
        },
        budget: ExecutionBudget {
            max_input_bytes: 64,
            max_output_bytes: 64,
            max_history_records: 1024,
            max_checkpoint_commits: 1024,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(10000),
        },
    }
}
fn task_scope() -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition().fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(&[7; 64])),
    }
}
fn replay_history(journal: &mut SqliteTaskJournal, expected: TaskProjection) -> Result<(), Error> {
    let mut actual = TaskProjection::new(task_scope(), definition())?;
    let mut blob = [0; 64];
    for sequence in 0..expected.next_sequence().get() {
        let record = journal
            .read(task_scope(), TaskSequence::new(sequence))?
            .ok_or("missing record")?;
        let reference = match record.event {
            TaskExecutionEvent::IntentCommitted { input, .. } => Some(input),
            TaskExecutionEvent::CheckpointCommitted { checkpoint } => Some(checkpoint),
            TaskExecutionEvent::CheckpointMigrated { .. }
            | TaskExecutionEvent::Started
            | TaskExecutionEvent::FaultObserved { .. }
            | TaskExecutionEvent::ReconciliationRequired
            | TaskExecutionEvent::Reconciled { .. }
            | TaskExecutionEvent::RecoveryPlanned { .. }
            | TaskExecutionEvent::Restarted
            | TaskExecutionEvent::Completed { .. }
            | TaskExecutionEvent::RecoveryExhausted => None,
        };
        if let Some(reference) = reference {
            let bytes = journal.read_blob(task_scope(), reference, &mut blob)?;
            reference.verify(blob.get(..bytes).ok_or("blob length")?, reference.schema)?;
        }
        actual.apply(&record)?;
    }
    if actual != expected {
        return Err("replayed projection differs".into());
    }
    Ok(())
}
fn task_case(directory: &Path, history_length: u32) -> Result<Value, Error> {
    let path = directory.join(format!("tasks-{history_length}.db"));
    let limits = SqliteLimits::default();
    let (mut journal, open_ns) =
        timed(|| Ok(SqliteTaskJournal::open(&path, definition(), limits)?))?;
    let bytes = [7; 64];
    let reference = BlobRef {
        digest: Digest::of(&bytes),
        bytes: 64,
        schema: 1,
    };
    let mut projection = TaskProjection::new(task_scope(), definition())?;
    let intent = projection.propose(
        TaskExecutionEvent::IntentCommitted {
            definition: definition(),
            input: reference,
        },
        TaskGeneration::ZERO,
        TaskTick::ZERO,
    )?;
    journal.commit(&intent, Some(&bytes))?;
    projection.apply(&intent)?;
    let started = projection.next(TaskTick::new(1))?.ok_or("missing start")?;
    journal.commit(&started, None)?;
    projection.apply(&started)?;
    let mut append = Vec::new();
    for sequence in 2..history_length {
        let record = projection.propose(
            TaskExecutionEvent::CheckpointCommitted {
                checkpoint: reference,
            },
            projection.generation(),
            TaskTick::new(u64::from(sequence)),
        )?;
        let (_, elapsed) = timed(|| Ok(journal.commit(&record, Some(&bytes))?))?;
        append.push(elapsed);
        projection.apply(&record)?;
    }
    let mut warm = Vec::new();
    let mut reopened = Vec::new();
    for _ in 0..10 {
        let ((), warm_ns) = timed(|| replay_history(&mut journal, projection))?;
        warm.push(warm_ns);
        let ((), reopen_ns) = timed(|| {
            let mut fresh = SqliteTaskJournal::open(&path, definition(), limits)?;
            replay_history(&mut fresh, projection)
        })?;
        reopened.push(reopen_ns);
    }
    journal.check_integrity()?;
    Ok(
        json!({"records": history_length, "payload_bytes": 64, "new_database_open_ns": open_ns, "checkpoint_append": distribution(append)?, "warm_record_and_blob_replay": distribution(warm)?, "reopen_and_record_and_blob_replay": distribution(reopened)?}),
    )
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn run<T>(future: impl Future<Output = Result<T, PortError>>) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Ok(value?);
        }
        if Instant::now() >= deadline {
            return Err("database worker timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(1));
    }
}
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
fn process_case(directory: &Path) -> Result<Value, Error> {
    let path = directory.join("process.db");
    let store =
        SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 10000, 8)?;
    let scope = ProcessScope::new(
        "tnt_bench".parse()?,
        "prc_bench".parse()?,
        "def_bench".parse()?,
        "dfv_one".parse()?,
        ContentDigest([9; 32]),
    );
    let request = OutboxClaimRequest::new(
        scope.clone(),
        "pri_bench".parse()?,
        NonZeroU16::new(1).ok_or("limit")?,
    )?;
    let mut commits = Vec::new();
    let mut retries = Vec::new();
    let mut claim_to_ack = Vec::new();
    for sequence in 0..128 {
        let input_id: InputId = format!("inp_{sequence}").parse()?;
        let input = ProcessInput::new(
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            input_id.clone(),
            ProcessInputKind::CanonicalEvent,
            ContentDigest([1; 32]),
        );
        let outcome = ProcessOutcome::new(
            scope.clone(),
            sequence,
            ProcessOutcomeFact::new(
                format!("out_{sequence}").parse()?,
                CausationId::Input(input_id),
                OutcomeActor::System,
                LogicalTime(sequence),
                ProcessOutcomeKind::InputAccepted,
                ContentDigest([2; 32]),
            ),
        );
        let action = ProcessAction::new(
            scope.clone(),
            format!("act_{sequence}").parse()?,
            "stp_bench".parse()?,
            u32::try_from(sequence)?,
            ProcessActionKind::ExternalEffect,
            ContentDigest([3; 32]),
        );
        let commit =
            AtomicProcessCommit::new(sequence, Some(input), vec![outcome], vec![action.clone()])?
                .with_canonical_source_event(format!("cev_{sequence}").parse()?);
        let (receipt, commit_ns) = timed(|| run(ProcessStore::commit(&store, &commit)))?;
        receipt.validate_for(&commit)?;
        commits.push(commit_ns);
        let ((), delivery_ns) = timed(|| {
            let leases = run(store.claim(&request))?;
            if leases.len() != 1 {
                return Err("expected one lease".into());
            }
            let lease = leases.first().ok_or("missing lease")?;
            if lease.record.action != action {
                return Err("different action".into());
            }
            run(OutboxStore::acknowledge(&store, lease, LogicalTime(100)))
        })?;
        claim_to_ack.push(delivery_ns);
        let (retry_receipt, retry_ns) = timed(|| run(ProcessStore::commit(&store, &commit)))?;
        retry_receipt.validate_for(&commit)?;
        if !retry_receipt.duplicate_input {
            return Err("retry was not deduplicated".into());
        }
        retries.push(retry_ns);
    }
    if !run(store.claim(&request))?.is_empty() {
        return Err("ACKed action redelivered".into());
    }
    Ok(
        json!({"commits": distribution(commits)?, "exact_retry_after_ack": distribution(retries)?, "claim_to_ack_without_external_execution": distribution(claim_to_ack)?}),
    )
}
fn main() -> Result<(), Error> {
    let directory = if let Some(parent) = std::env::var_os("PENELOPE_BENCH_DIR") {
        tempfile::tempdir_in(parent)?
    } else {
        tempfile::tempdir()?
    };
    let started = Instant::now();
    let mut tasks = Vec::new();
    for length in [16, 128, 512] {
        tasks.push(task_case(directory.path(), length)?);
    }
    let process = process_case(directory.path())?;
    let report = json!({"schema": 1, "sqlite_version": rusqlite::version(), "journal_mode": "WAL", "synchronous": "FULL", "cache": "OS cache is uncontrolled; warm and reopened-connection runs only", "checkpoint_payload": "one deduplicated immutable 64-byte blob", "tasks": tasks, "process": process, "total_wall_ns": started.elapsed().as_nanos()});
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
