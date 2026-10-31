use penelope_core::task::{runtime::*, *};
use penelope_executor::task::TaskSupervisor;
use penelope_process_runtime::{LinuxProcessRuntime, ProcessRuntimeConfig};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;
const INPUT: [u8; 4] = [5, 1, 2, 3];
struct Runtime {
    inner: LinuxProcessRuntime<SqliteTaskJournal>,
    checkpoint_ack: Option<Instant>,
    starts: u32,
    acks: u32,
}
impl TaskRuntime for Runtime {
    fn start(&mut self, request: TaskStart<'_>) -> Result<(), TaskPortError> {
        self.inner.start(request)?;
        self.starts = self.starts.checked_add(1).ok_or(TaskPortError::Exhausted)?;
        Ok(())
    }
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError> {
        self.inner.poll(scope, generation, buffer)
    }
    fn acknowledge_checkpoint(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        checkpoint: BlobRef,
    ) -> Result<(), TaskPortError> {
        self.inner
            .acknowledge_checkpoint(scope, generation, checkpoint)?;
        self.checkpoint_ack = Some(Instant::now());
        self.acks = self.acks.checked_add(1).ok_or(TaskPortError::Exhausted)?;
        Ok(())
    }
    fn fence(&mut self, scope: TaskScope, generation: TaskGeneration) -> Result<(), TaskPortError> {
        self.inner.fence(scope, generation)
    }
}
const fn definition(code: Digest) -> TaskDefinition {
    TaskDefinition {
        code,
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromCheckpoint,
            max_attempts: 2,
            timeout_ticks: 30,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 64,
        },
        budget: ExecutionBudget {
            max_input_bytes: 64,
            max_output_bytes: 64,
            max_history_records: 16,
            max_checkpoint_commits: 2,
            cpu_ticks: 2,
            deadline_tick: TaskTick::new(1000),
        },
    }
}
fn scope(definition: TaskDefinition, index: u32) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest::of(&index.to_be_bytes())),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(&INPUT)),
    }
}
fn worker(directory: &Path) -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os("PENELOPE_DISPATCH_WORKER") {
        return Ok(path.into());
    }
    let binary = directory.join("worker");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/worker.c");
    let status = Command::new("cc")
        .args([
            "-nostdlib",
            "-static",
            "-fno-builtin",
            "-fno-stack-protector",
            "-fno-pie",
            "-no-pie",
            "-o",
        ])
        .arg(&binary)
        .arg(source)
        .status()?;
    if !status.success() {
        return Err("worker compilation failed".into());
    }
    Ok(binary)
}
fn open(
    directory: &Path,
    worker: &Path,
    definition: TaskDefinition,
) -> Result<(SqliteTaskJournal, Runtime), Error> {
    std::fs::create_dir_all(directory)?;
    let path = directory.join("tasks.db");
    let journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default())?;
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default())?;
    let inner = LinuxProcessRuntime::new(
        authority,
        ProcessRuntimeConfig {
            registry: directory.join("registry"),
            worker: worker.to_path_buf(),
            max_worker_bytes: 1024 * 1024,
            max_frame_bytes: 64,
            max_live: 1,
            ticks_per_second: 1,
            address_space_bytes: 64 * 1024 * 1024,
            startup_timeout: Duration::from_secs(1),
            fence_timeout: Duration::from_secs(1),
        },
    )?;
    Ok((
        journal,
        Runtime {
            inner,
            checkpoint_ack: None,
            starts: 0,
            acks: 0,
        },
    ))
}
fn case(
    journal: SqliteTaskJournal,
    mut runtime: Runtime,
    definition: TaskDefinition,
    index: u32,
) -> Result<(SqliteTaskJournal, Runtime, Value), Error> {
    runtime.checkpoint_ack = None;
    runtime.starts = 0;
    runtime.acks = 0;
    let scope = scope(definition, index);
    let mut supervisor: TaskSupervisor<_, _, 64> =
        TaskSupervisor::restore(journal, runtime, scope, definition, TaskTick::ZERO)?;
    let intent_start = Instant::now();
    supervisor.submit(&INPUT, 1, TaskTick::ZERO)?;
    let intent_ns = intent_start.elapsed().as_nanos();
    // Dispatch includes the Started commit, blob read, generation reservation,
    // manifest fsync, fork/exec and bounded start-frame write. Intent is separate.
    let dispatched = Instant::now();
    supervisor.advance(TaskTick::new(1))?;
    let start_return_ns = dispatched.elapsed().as_nanos();
    let deadline = dispatched
        .checked_add(Duration::from_secs(5))
        .ok_or("deadline overflow")?;
    while supervisor.projection().status() == TaskStatus::Running {
        let tick = dispatched
            .elapsed()
            .as_secs()
            .checked_add(2)
            .ok_or("clock overflow")?;
        supervisor.poll(TaskTick::new(tick))?;
        if Instant::now() >= deadline {
            return Err("worker benchmark timeout".into());
        }
        if supervisor.projection().status() == TaskStatus::Running {
            std::thread::yield_now();
        }
    }
    let completed_ns = dispatched.elapsed().as_nanos();
    if supervisor.projection().status() != TaskStatus::Completed {
        return Err("worker did not complete".into());
    }
    let checkpoint = supervisor
        .projection()
        .checkpoint()
        .ok_or("missing checkpoint")?;
    let output = supervisor.projection().output().ok_or("missing output")?;
    let (mut finished_journal, finished_runtime) = supervisor.into_parts();
    if finished_runtime.starts != 1 || finished_runtime.acks != 1 {
        return Err("wrong execution/ACK count".into());
    }
    let ack_ns = finished_runtime
        .checkpoint_ack
        .ok_or("missing host ACK")?
        .duration_since(dispatched)
        .as_nanos();
    if ack_ns < start_return_ns || ack_ns > completed_ns {
        return Err("invalid timing order".into());
    }
    let mut bytes = [0; 64];
    for reference in [checkpoint, output] {
        let length = finished_journal.read_blob(scope, reference, &mut bytes)?;
        if bytes.get(..length) != Some(6_u64.to_le_bytes().as_slice()) {
            return Err("wrong checkpoint/output bytes".into());
        }
    }
    // Validate authoritative history after timing, without changing runtime
    // ownership or adding replay work to the measured completion interval.
    let mut projection = TaskProjection::new(scope, definition)?;
    while let Some(record) = finished_journal.read(scope, projection.next_sequence())? {
        projection.apply(&record)?;
    }
    if projection.status() != TaskStatus::Completed
        || projection.output() != Some(output)
        || projection.checkpoint() != Some(checkpoint)
    {
        return Err("durable replay mismatch".into());
    }
    Ok((
        finished_journal,
        finished_runtime,
        json!({"index":index,"intent_commit_ns":intent_ns,"dispatch_start_return_ns":start_return_ns,"dispatch_to_host_checkpoint_ack_ns":ack_ns,"dispatch_to_durable_completion_and_fence_ns":completed_ns}),
    ))
}
fn distribution(rows: &[Value], field: &str) -> Result<Value, Error> {
    let mut samples = rows
        .iter()
        .map(|row| {
            row[field]
                .as_u64()
                .ok_or_else(|| Error::from("invalid duration"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    samples.sort_unstable();
    let percentile = |p: usize| -> Result<u64, Error> {
        let rank = samples
            .len()
            .checked_mul(p)
            .ok_or("rank overflow")?
            .div_ceil(100);
        samples
            .get(rank.saturating_sub(1))
            .copied()
            .ok_or_else(|| "empty samples".into())
    };
    Ok(
        json!({"samples":rows.len(),"p50_ns":percentile(50)?,"p95_ns":percentile(95)?,"p99_ns":percentile(99)?}),
    )
}
pub(super) fn run() -> Result<(), Error> {
    let samples =
        std::env::var("PENELOPE_DISPATCH_SAMPLES").map_or(Ok(2), |value| value.parse::<u32>())?;
    if !(1..=1024).contains(&samples) {
        return Err("samples must be 1..=1024".into());
    }
    let directory = if let Some(parent) = std::env::var_os("PENELOPE_BENCH_DIR") {
        tempfile::tempdir_in(parent)?
    } else {
        tempfile::tempdir()?
    };
    let worker = worker(directory.path())?;
    let definition = definition(Digest::of(&std::fs::read(&worker)?));
    let mut profiles = Vec::new();
    for reused in [false, true] {
        let mut connection = None;
        let mut rows = Vec::new();
        let count = samples
            .checked_add(u32::from(reused))
            .ok_or("sample count overflow")?;
        for index in 0..count {
            let setup = Instant::now();
            let (journal, runtime) = if let Some(ports) = connection.take() {
                ports
            } else {
                open(
                    &directory.path().join(if reused {
                        "reused".into()
                    } else {
                        format!("fresh-{index}")
                    }),
                    &worker,
                    definition,
                )?
            };
            let setup_ns = setup.elapsed().as_nanos();
            let (journal, runtime, mut row) = case(journal, runtime, definition, index)?;
            row.as_object_mut()
                .ok_or("sample is not an object")?
                .insert("setup_ns".into(), json!(setup_ns));
            row.as_object_mut()
                .ok_or("sample is not an object")?
                .insert("warmup".into(), json!(reused && index == 0));
            rows.push(row);
            if reused {
                connection = Some((journal, runtime));
            }
        }
        let measured = if reused {
            rows.get(1..).ok_or("missing warmup")?
        } else {
            rows.as_slice()
        };
        let mut distributions = serde_json::Map::new();
        for field in [
            "setup_ns",
            "intent_commit_ns",
            "dispatch_start_return_ns",
            "dispatch_to_host_checkpoint_ack_ns",
            "dispatch_to_durable_completion_and_fence_ns",
        ] {
            distributions.insert(field.into(), distribution(measured, field)?);
        }
        profiles.push(json!({"profile":if reused {"reused_runtime_and_store"} else {"fresh_runtime_and_store"},"samples":rows,"distributions":distributions}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"schema":1,"journal_mode":"WAL","synchronous":"FULL","worker_input":INPUT,"worker_output_u64":6,"worker_code_sha256":definition.code.0,"polling":"yield_now between pending polls; monotonic whole-second task ticks","cache":"OS cache uncontrolled; fresh profile is not page-cache-cold","ack_endpoint":"successful host checkpoint ACK write, not a worker/application receipt","profiles":profiles})
        )?
    );
    Ok(())
}
