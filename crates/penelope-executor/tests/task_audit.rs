//! Deep-audit regressions at execution admission and observation boundaries.
#![allow(clippy::unwrap_used)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::{runtime::*, *};
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use support::*;

type Supervisor = TaskSupervisor<MemoryJournal, SumRuntime, 64>;
const INPUT: [u8; 64] = [1; 64];

fn start(history: u32) -> Supervisor {
    let mut definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    definition.budget.max_history_records = history;
    let mut supervisor = Supervisor::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope(definition, &INPUT),
        definition,
        TaskTick::new(0),
    )
    .unwrap();
    supervisor.submit(&INPUT, 1, TaskTick::new(0)).unwrap();
    supervisor.advance(TaskTick::new(1)).unwrap();
    supervisor
}

#[test]
fn last_history_slot_is_reserved_for_stopping_the_generation() {
    let mut supervisor = start(4);
    supervisor.poll(TaskTick::new(2)).unwrap();
    assert_eq!(
        supervisor.poll(TaskTick::new(3)),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(
        supervisor.projection().next_sequence(),
        TaskSequence::new(3)
    );
    assert_eq!(
        supervisor.poll(TaskTick::new(4)),
        Err(SupervisorError::ReconstructionRequired)
    );
    let definition = supervisor.projection().definition();
    let scope = supervisor.projection().scope();
    let (journal, runtime) = supervisor.into_parts();
    assert_eq!(runtime.fences, 1);
    let recovered =
        Supervisor::restore(journal, runtime, scope, definition, TaskTick::new(4)).unwrap();
    assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
    assert_eq!(recovered.projection().next_sequence(), TaskSequence::new(4));
    assert!(recovered.projection().output().is_none());
}

#[test]
fn restart_without_a_terminal_history_slot_never_dispatches() {
    let mut supervisor = start(5);
    supervisor
        .observe(
            TaskObservation {
                scope: supervisor.projection().scope(),
                generation: TaskGeneration::new(1),
                progress: RuntimeProgress::Fault(TaskFault::Crash),
            },
            &[],
            TaskTick::new(2),
        )
        .unwrap();
    supervisor.advance(TaskTick::new(3)).unwrap();
    assert_eq!(
        supervisor.advance(TaskTick::new(4)),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(supervisor.projection().status(), TaskStatus::Ready);
    assert_eq!(supervisor.projection().generation(), TaskGeneration::new(1));
    supervisor.advance(TaskTick::new(10000)).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Exhausted);
    assert_eq!(supervisor.into_parts().1.starts, 1);
}

#[test]
fn forwarded_pending_observation_at_timeout_stops_execution() {
    let mut supervisor = start(20);
    let scope = supervisor.projection().scope();
    assert_eq!(
        supervisor.observe(
            TaskObservation {
                scope,
                generation: TaskGeneration::new(1),
                progress: RuntimeProgress::Pending,
            },
            &[],
            TaskTick::new(51)
        ),
        Err(SupervisorError::Decision(TaskError::DeadlineExpired))
    );
    assert_eq!(supervisor.projection().status(), TaskStatus::Faulted);
    assert!(supervisor.projection().output().is_none());
    assert_eq!(supervisor.into_parts().1.fences, 1);
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn quota_fence_failure_poisoning_never_acknowledges_a_checkpoint() {
    let mut definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    definition.budget.max_checkpoint_commits = 0;
    let mut runtime = SumRuntime::default();
    runtime.fence_failure = true;
    let mut supervisor = Supervisor::restore(
        MemoryJournal::default(),
        runtime,
        scope(definition, &INPUT),
        definition,
        TaskTick::new(0),
    )
    .unwrap();
    supervisor.submit(&INPUT, 1, TaskTick::new(0)).unwrap();
    supervisor.advance(TaskTick::new(1)).unwrap();
    assert_eq!(
        supervisor.poll(TaskTick::new(2)),
        Err(SupervisorError::Port(TaskPortError::Conflict))
    );
    assert_eq!(
        supervisor.poll(TaskTick::new(3)),
        Err(SupervisorError::ReconstructionRequired)
    );
    assert_eq!(
        supervisor.projection().next_sequence(),
        TaskSequence::new(2)
    );
    assert!(supervisor.projection().checkpoint().is_none());
}

#[test]
fn foreign_or_stale_late_observations_cannot_fence_the_current_generation() {
    let mut supervisor = start(20);
    let mut foreign = supervisor.projection().scope();
    foreign.task = TaskId::from_digest(Digest([9; 32]));
    for (scope, generation, expected) in [
        (foreign, 1, TaskError::ScopeMismatch),
        (
            supervisor.projection().scope(),
            0,
            TaskError::StaleGeneration,
        ),
    ] {
        assert_eq!(
            supervisor.observe(
                TaskObservation {
                    scope,
                    generation: TaskGeneration::new(generation),
                    progress: RuntimeProgress::Pending,
                },
                &[],
                TaskTick::new(51)
            ),
            Err(SupervisorError::Decision(expected))
        );
    }
    assert_eq!(supervisor.projection().status(), TaskStatus::Running);
    assert_eq!(supervisor.into_parts().1.fences, 0);
}

#[test]
fn persisted_input_schema_reaches_the_runtime_before_computation() {
    let definition = definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let mut supervisor = Supervisor::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope(definition, &INPUT),
        definition,
        TaskTick::new(0),
    )
    .unwrap();
    supervisor.submit(&INPUT, 2, TaskTick::new(0)).unwrap();
    assert_eq!(
        supervisor.advance(TaskTick::new(1)),
        Err(SupervisorError::Port(TaskPortError::Corrupt))
    );
    assert_eq!(
        supervisor.advance(TaskTick::new(2)),
        Err(SupervisorError::ReconstructionRequired)
    );
    assert!(supervisor.projection().output().is_none());
    assert_eq!(supervisor.into_parts().1.starts, 0);
}

#[test]
fn oversized_reports_stop_execution_even_when_the_buffer_is_truncated() {
    for bytes in [65, u32::MAX] {
        for progress in [
            RuntimeProgress::Checkpoint { bytes, schema: 1 },
            RuntimeProgress::Complete { bytes, schema: 1 },
        ] {
            for buffer in [&[][..], &INPUT[..]] {
                let mut supervisor = start(20);
                let before = *supervisor.projection();
                assert_eq!(
                    supervisor.observe(
                        TaskObservation {
                            scope: before.scope(),
                            generation: before.generation(),
                            progress,
                        },
                        buffer,
                        TaskTick::new(2),
                    ),
                    Err(SupervisorError::Decision(TaskError::QuotaExhausted))
                );
                assert_eq!(*supervisor.projection(), before);
                assert_eq!(
                    supervisor.poll(TaskTick::new(3)),
                    Err(SupervisorError::ReconstructionRequired)
                );
                let (journal, runtime) = supervisor.into_parts();
                assert_eq!(journal.records.len(), 2);
                assert_eq!(journal.blobs.len(), 1); // Only immutable input committed.
                assert_eq!(runtime.fences, 1);
                let recovered = Supervisor::restore(
                    journal,
                    runtime,
                    before.scope(),
                    before.definition(),
                    TaskTick::new(3),
                )
                .unwrap();
                assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
                assert!(recovered.projection().checkpoint().is_none());
                assert!(recovered.projection().output().is_none());
            }
        }
    }
}
