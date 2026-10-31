use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
use serde_json::json;
use std::time::{Duration, Instant};
#[allow(clippy::unwrap_used)]
#[path = "../../tests/support/runtime_fixture.rs"]
mod fixture;
type Error = Box<dyn std::error::Error>;
fn case(profile: &str, definition: TaskDefinition) -> Result<u64, Error> {
    let directory = tempfile::tempdir()?;
    let input = [if profile == "exited" { 7 } else { 9 }];
    let scope = TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(&input)),
    };
    let path = directory.path().join("tasks.db");
    let journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default())?;
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default())?;
    let initial_runtime = fixture::Runtime::new(authority, fixture::config(directory.path()))?;
    let mut supervisor =
        fixture::Supervisor::restore(journal, initial_runtime, scope, definition, TaskTick::ZERO)?;
    supervisor.submit(&input, 1, TaskTick::ZERO)?;
    supervisor.advance(TaskTick::new(1))?;
    let projection = *supervisor.projection();
    let (_journal, mut runtime) = supervisor.into_parts();
    if profile == "exited" {
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(5))
            .ok_or("deadline overflow")?;
        loop {
            match runtime
                .poll(scope, projection.generation(), &mut [0; 64])?
                .progress
            {
                RuntimeProgress::Pending => {}
                RuntimeProgress::Fault(TaskFault::ApplicationFailure) => break,
                RuntimeProgress::Checkpoint { .. }
                | RuntimeProgress::Complete { .. }
                | RuntimeProgress::Fault(_) => {
                    return Err("unexpected exited-worker progress".into());
                }
            }
            if Instant::now() >= deadline {
                return Err("worker did not exit".into());
            }
            std::thread::yield_now();
        }
    } else if profile == "repeated" {
        runtime.fence(scope, projection.generation())?;
    }
    let began = Instant::now();
    runtime.fence(scope, projection.generation())?;
    let elapsed = u64::try_from(began.elapsed().as_nanos())?;
    if runtime.poll(scope, projection.generation(), &mut [0; 64]) != Err(TaskPortError::Conflict) {
        return Err("fenced worker still active".into());
    }
    if runtime.start(TaskStart {
        scope,
        generation: projection.generation(),
        definition,
        execution_deadline: projection.execution_deadline(),
        input_schema: 1,
        input: &input,
        checkpoint: None,
    }) != Err(TaskPortError::Conflict)
    {
        return Err("fenced generation can restart".into());
    }
    Ok(elapsed)
}
pub(super) fn run() -> Result<(), Error> {
    let samples = std::env::var("PENELOPE_FENCE_SAMPLES").map_or(Ok(2), |v| v.parse::<u32>())?;
    if !(1..=1024).contains(&samples) {
        return Err("samples must be 1..=1024".into());
    }
    let definition = fixture::definition(2);
    let mut profiles = serde_json::Map::new();
    for profile in ["live", "exited", "repeated"] {
        let mut rows = Vec::new();
        for _ in 0..samples {
            rows.push(case(profile, definition)?);
        }
        profiles.insert(profile.into(), json!(rows));
    }
    println!(
        "{}",
        serde_json::to_string(
            &json!({"schema":1,"samples_per_profile":samples,"profiles_ns":profiles,"scope":"Public fence includes registry lock, WAL/FULL begin/confirm and kernel termination/reaping. Fresh fixture/worker startup and post-fence validation excluded. Exited worker and repeated fence controls."})
        )?
    );
    Ok(())
}
