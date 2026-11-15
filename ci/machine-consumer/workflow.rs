//! Optional Linux execution leg. Uses the production supervisor and fenced runtime.
use penelope::task::{
    runtime::{TaskJournal, TaskPortError},
    *,
};
use penelope::{machine::*, storage::machine::MachineTaskBinding};
use penelope_process_runtime::{LinuxProcessRuntime, ProcessRuntimeConfig};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn invalid(message: &str) -> std::io::Error {
    std::io::Error::other(message)
}

pub fn pin_worker(
    mut definition: TaskDefinition,
    worker: Option<PathBuf>,
) -> Result<(TaskDefinition, PathBuf)> {
    let worker = worker.ok_or_else(|| {
        invalid("pass the worker path; see scripts/run-supervised-saga-example.sh")
    })?;
    let mut bytes = Vec::new();
    std::fs::File::open(&worker)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("worker exceeds image bound").into());
    }
    definition.code = Digest::of(&bytes);
    definition.recovery.timeout_ticks = 10_000;
    definition.budget.deadline_tick = TaskTick::new(20_000);
    definition.budget.cpu_ticks = 2000;
    Ok((definition, worker))
}

pub struct CompletedCount {
    pub amount: u64,
    pub event: MachineEventId,
    pub causation: MachineCausation,
}

pub fn complete(
    root: &Path,
    worker: &Path,
    definition: TaskDefinition,
    binding: MachineTaskBinding,
    observe: &mut impl FnMut(&'static str),
) -> Result<CompletedCount> {
    let scope = binding.scope();
    let path = root.join("tasks.db");
    let journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default())?;
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default())?;
    let runtime = LinuxProcessRuntime::new(
        authority,
        ProcessRuntimeConfig {
            registry: root.join("workers"),
            worker: worker.to_path_buf(),
            max_worker_bytes: 1024 * 1024,
            max_frame_bytes: 64,
            max_live: 1,
            ticks_per_second: 1000,
            address_space_bytes: 64 * 1024 * 1024,
            startup_timeout: Duration::from_secs(2),
            fence_timeout: Duration::from_secs(2),
        },
    )?;
    let mut supervisor =
        TaskSupervisor::<_, _, 8>::restore(journal, runtime, scope, definition, TaskTick::new(3))?;
    observe("native_setup");
    // Generation zero is already admitted by the receiver. Do not submit it again.
    let start = Instant::now();
    for _ in 0..5000 {
        let tick = TaskTick::new(
            4_u64
                .checked_add(u64::try_from(start.elapsed().as_millis())?)
                .ok_or_else(|| invalid("task clock overflow"))?,
        );
        match supervisor.projection().status() {
            TaskStatus::Completed => {
                observe("durable_completed");
                break;
            }
            TaskStatus::Running => {
                supervisor.poll(tick)?;
            }
            TaskStatus::Pending => {
                supervisor.advance(tick)?;
                observe("dispatch_return");
            }
            _ => return Err(invalid("task unresolved; no result event admitted").into()),
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let tick = supervisor.projection().last_tick();
    let (journal, runtime) = supervisor.into_parts();
    // Reconstruct from authoritative persisted history before using the output.
    let restored = TaskSupervisor::<_, _, 8>::restore(journal, runtime, scope, definition, tick)?;
    let projection = *restored.projection();
    if projection.status() != TaskStatus::Completed || definition.effects != EffectContract::Pure {
        return Err(invalid("task completion is not established").into());
    }
    let output = projection
        .output()
        .ok_or_else(|| invalid("missing completed output"))?;
    let (mut journal, _runtime) = restored.into_parts();
    let mut bytes = [0; 8];
    let len = journal.read_blob(scope, output, &mut bytes)?;
    // This application pins the worker's schema and meaning: sum the input bytes
    // after its mode byte. Counter value 256 encodes [0,1,0,0,0,0,0,0], hence 1.
    // Completion of effectful tasks would additionally need independent evidence.
    if output.bytes != 8 || len != 8 || Digest::of(&bytes) != output.digest {
        return Err(TaskPortError::Corrupt.into());
    }
    if output.schema != 1 || u64::from_le_bytes(bytes) != 1 {
        return Err(
            invalid("task result schema or meaning differs; no result event admitted").into(),
        );
    }
    observe("result_verified");
    println!("task_result_verified=true amount=1");
    let mut identity = b"penelope.counter-result.v1\0".to_vec();
    identity.extend_from_slice(&scope.task.digest().0);
    identity.extend_from_slice(&projection.generation().get().to_le_bytes());
    identity.extend_from_slice(&projection.next_sequence().get().to_le_bytes());
    identity.extend_from_slice(&output.digest.0);
    let cause = Digest::of(&identity).0;
    Ok(CompletedCount {
        amount: u64::from_le_bytes(bytes),
        event: MachineEventId::from_bytes(cause),
        causation: MachineCausation {
            source: MachineSourceId::from_bytes(scope.task.digest().0),
            cause: MachineCauseId::from_bytes(cause),
            correlation: MachineCorrelationId::from_bytes(
                binding.source().binding_identity(binding.consumer()),
            ),
        },
    })
}
