//! Isolated pending-task polling through portable and hosted public interfaces.
#[path = "../tests/support/tasks.rs"]
mod support;
use penelope_core::task::{control::TaskControl, runtime::*, *};
use penelope_executor::task::TaskSupervisor;
use std::{hint::black_box, time::Instant};
type Error = Box<dyn std::error::Error>;
struct PendingRuntime;
impl TaskRuntime for PendingRuntime {
    fn start(&mut self, _request: TaskStart<'_>) -> Result<(), TaskPortError> {
        Ok(())
    }
    #[inline(never)]
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        _buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError> {
        Ok(black_box(TaskObservation {
            scope,
            generation,
            progress: RuntimeProgress::Pending,
        }))
    }
    fn acknowledge_checkpoint(
        &mut self,
        _scope: TaskScope,
        _generation: TaskGeneration,
        _checkpoint: BlobRef,
    ) -> Result<(), TaskPortError> {
        Ok(())
    }
    fn fence(
        &mut self,
        _scope: TaskScope,
        _generation: TaskGeneration,
    ) -> Result<(), TaskPortError> {
        Ok(())
    }
}
fn fixture(capacity: u32) -> (TaskDefinition, TaskScope) {
    let mut definition =
        support::definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    definition.budget.max_input_bytes = capacity;
    definition.budget.max_output_bytes = capacity;
    definition.recovery.max_checkpoint_bytes = capacity;
    (definition, support::scope(definition, b"poll"))
}
fn measure(
    name: &str,
    iterations: u32,
    mut operation: impl FnMut() -> Result<bool, Error>,
) -> Result<(), Error> {
    if operation()? {
        return Err("pending operation advanced history".into());
    }
    let began = Instant::now();
    for _ in 0..iterations {
        if black_box(operation()?) {
            return Err("pending operation advanced history".into());
        }
    }
    let elapsed = began.elapsed();
    let nanos = elapsed
        .as_nanos()
        .checked_div(u128::from(iterations))
        .ok_or("zero iterations")?;
    println!("{name}: {iterations} operations in {elapsed:?} ({nanos} ns/op)");
    Ok(())
}
fn portable(capacity: u32) -> Result<(), Error> {
    let (definition, scope) = fixture(capacity);
    let mut control = TaskControl::restore(
        support::MemoryJournal::default(),
        PendingRuntime,
        scope,
        definition,
        TaskTick::ZERO,
    )?;
    control.submit(b"poll", 1, TaskTick::ZERO)?;
    let length = usize::try_from(capacity)?;
    let (mut input, mut output, mut checkpoint) =
        (vec![0; length], vec![0; length], vec![0; length]);
    control.advance(
        TaskTick::new(1),
        &mut penelope_core::task::control::TaskWorkspace {
            input: &mut input,
            output: &mut output,
            checkpoint: &mut checkpoint,
        },
    )?;
    measure(
        &format!("portable pending capacity={capacity}"),
        2_000_000,
        || {
            Ok(black_box(&mut control)
                .poll_with_buffer(black_box(TaskTick::new(2)), black_box(&mut output))?)
        },
    )?;
    let observation = TaskObservation {
        scope,
        generation: control.projection().generation(),
        progress: RuntimeProgress::Pending,
    };
    measure(
        &format!("observe pending capacity={capacity}"),
        2_000_000,
        || {
            Ok(black_box(&mut control).observe(
                black_box(observation),
                black_box(&output),
                black_box(TaskTick::new(2)),
            )?)
        },
    )
}
fn hosted<const N: usize>() -> Result<(), Error> {
    let (definition, scope) = fixture(u32::try_from(N)?);
    let mut supervisor = TaskSupervisor::<_, _, N>::restore(
        support::MemoryJournal::default(),
        PendingRuntime,
        scope,
        definition,
        TaskTick::ZERO,
    )?;
    supervisor.submit(b"poll", 1, TaskTick::ZERO)?;
    supervisor.advance(TaskTick::new(1))?;
    let iterations = if N >= 65_536 { 20_000 } else { 2_000_000 };
    measure(&format!("hosted pending capacity={N}"), iterations, || {
        Ok(black_box(&mut supervisor).poll(black_box(TaskTick::new(2)))?)
    })
}
fn main() -> Result<(), Error> {
    for capacity in [64, 4096, 65_536] {
        portable(capacity)?;
    }
    hosted::<64>()?;
    hosted::<4096>()?;
    hosted::<65_536>()
}
