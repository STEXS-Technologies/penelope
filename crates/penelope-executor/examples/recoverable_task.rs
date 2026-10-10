//! Demonstration only: application-owned memory journal and simulated runtime.
#[path = "../tests/support/tasks.rs"]
mod support;
use penelope_core::task::{EffectContract, RecoveryMode, TaskStatus};
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use support::{MemoryJournal, SumRuntime, definition, scope};

fn main() -> Result<(), SupervisorError> {
    let input = [1; 64];
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &input);
    let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        0,
    )?;
    supervisor.submit(&input, 1, 0)?;
    supervisor.advance(1)?;
    supervisor.poll(2)?; // First eight records checkpoint atomically.
    let (journal, runtime) = supervisor.into_parts();
    // Reconstruct after interruption. Old generation is fenced by the adapter.
    let mut restored = TaskSupervisor::<_, _, 64>::restore(journal, runtime, scope, definition, 3)?;
    for tick in 4..30 {
        if restored.projection().status() == TaskStatus::Completed {
            break;
        }
        if restored.projection().status() == TaskStatus::Running {
            restored.poll(tick)?;
        } else {
            restored.advance(tick)?;
        }
    }
    println!(
        "Task {:?}, generation {}",
        restored.projection().status(),
        restored.projection().generation()
    );
    let (_journal, recovered_runtime) = restored.into_parts();
    println!("Resume offsets: {:?}", recovered_runtime.resumed);
    Ok(())
}
