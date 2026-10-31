//! Portable/hosted parity at durable and runtime boundaries.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
#[path = "support/tasks.rs"]
mod support;

use penelope_core::task::{control::*, *};
use penelope_executor::task::TaskSupervisor;
use support::*;

#[test]
fn portable_and_hosted_histories_match_through_checkpoint_and_completion() {
    let input = [1; 64];
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &input);
    let mut portable = TaskControl::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    let mut hosted = TaskSupervisor::<_, _, 64>::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    let mut input_buffer = [0; 64];
    let mut output = [0; 64];
    let mut checkpoint = [0; 64];
    let mut workspace = TaskWorkspace {
        input: &mut input_buffer,
        output: &mut output,
        checkpoint: &mut checkpoint,
    };
    portable.submit(&input, 1, TaskTick::ZERO).unwrap();
    hosted.submit(&input, 1, TaskTick::ZERO).unwrap();
    for tick in 1..20 {
        let tick = TaskTick::new(tick);
        let (left, right) = if portable.projection().status() == TaskStatus::Running {
            (portable.poll(tick, &mut workspace), hosted.poll(tick))
        } else {
            (portable.advance(tick, &mut workspace), hosted.advance(tick))
        };
        assert_eq!(left, right);
        assert_eq!(portable.projection(), hosted.projection());
        if portable.projection().status() == TaskStatus::Completed {
            break;
        }
    }
    assert_eq!(portable.projection().status(), TaskStatus::Completed);
    let (left, left_runtime) = portable.into_parts();
    let (right, right_runtime) = hosted.into_parts();
    assert_eq!(left.records, right.records);
    assert_eq!(left.blobs, right.blobs);
    assert_eq!(left_runtime.starts, right_runtime.starts);
    assert_eq!(left_runtime.fences, right_runtime.fences);
}

#[test]
fn insufficient_workspace_does_not_commit_start_and_can_be_retried() {
    let definition = definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let scope = scope(definition, &[1]);
    let mut control = TaskControl::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    control.submit(&[1], 1, TaskTick::ZERO).unwrap();
    let before = *control.projection();
    let mut input = [0; 64];
    let mut output = [0; 64];
    let mut checkpoint = [0; 63];
    let mut workspace = TaskWorkspace {
        input: &mut input,
        output: &mut output,
        checkpoint: &mut checkpoint,
    };
    assert_eq!(
        control.advance(TaskTick::new(1), &mut workspace),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(control.projection(), &before);
    let (journal, runtime) = control.into_parts();
    assert_eq!(journal.records.len(), 1);
    assert_eq!(runtime.starts, 0);
}

#[test]
fn ambiguous_commit_poisoning_matches_hosted_profile() {
    for cut in [
        Cut::BeforeCommit,
        Cut::AfterCommit,
        Cut::WrongReceipt,
        Cut::StorageFull,
    ] {
        let definition = definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
        let scope = scope(definition, &[1]);
        let journal = MemoryJournal {
            cut: Some(cut),
            ..MemoryJournal::default()
        };
        let mut portable = TaskControl::restore(
            journal.clone(),
            SumRuntime::default(),
            scope,
            definition,
            TaskTick::ZERO,
        )
        .unwrap();
        let mut hosted = TaskSupervisor::<_, _, 64>::restore(
            journal,
            SumRuntime::default(),
            scope,
            definition,
            TaskTick::ZERO,
        )
        .unwrap();
        assert_eq!(
            portable.submit(&[1], 1, TaskTick::ZERO),
            hosted.submit(&[1], 1, TaskTick::ZERO)
        );
        assert_eq!(
            portable.submit(&[1], 1, TaskTick::new(1)),
            Err(SupervisorError::ReconstructionRequired)
        );
        assert_eq!(portable.projection(), hosted.projection());
        assert_eq!(
            portable.into_parts().0.records,
            hosted.into_parts().0.records
        );
    }
}

#[test]
fn undersized_poll_workspace_preserves_worker_progress() {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &[1]);
    let mut control = TaskControl::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    let mut input = [0; 64];
    let mut output = [0; 64];
    let mut checkpoint = [0; 64];
    control.submit(&[1], 1, TaskTick::ZERO).unwrap();
    control
        .advance(
            TaskTick::new(1),
            &mut TaskWorkspace {
                input: &mut input,
                output: &mut output,
                checkpoint: &mut checkpoint,
            },
        )
        .unwrap();
    let before = *control.projection();
    assert_eq!(
        control.poll(
            TaskTick::new(2),
            &mut TaskWorkspace {
                input: &mut input,
                output: &mut output[..63],
                checkpoint: &mut checkpoint
            }
        ),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(control.projection(), &before);
    assert!(
        control
            .poll(
                TaskTick::new(2),
                &mut TaskWorkspace {
                    input: &mut input,
                    output: &mut output,
                    checkpoint: &mut checkpoint
                }
            )
            .unwrap()
    );
    assert!(control.projection().checkpoint().is_some());
}

#[test]
fn reconstruction_and_restart_match_hosted_profile() {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &[1; 64]);
    let mut initial_hosted = TaskSupervisor::<_, _, 64>::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    initial_hosted.submit(&[1; 64], 1, TaskTick::ZERO).unwrap();
    initial_hosted.advance(TaskTick::new(1)).unwrap();
    initial_hosted.poll(TaskTick::new(2)).unwrap();
    let (journal, runtime) = initial_hosted.into_parts();
    let mut portable = TaskControl::restore(
        journal.clone(),
        runtime.clone(),
        scope,
        definition,
        TaskTick::new(3),
    )
    .unwrap();
    let mut hosted =
        TaskSupervisor::<_, _, 64>::restore(journal, runtime, scope, definition, TaskTick::new(3))
            .unwrap();
    let mut input = [0; 64];
    let mut output = [0; 64];
    let mut checkpoint = [0; 64];
    let mut workspace = TaskWorkspace {
        input: &mut input,
        output: &mut output,
        checkpoint: &mut checkpoint,
    };
    for tick in 4..20 {
        let tick = TaskTick::new(tick);
        let (left, right) = if portable.projection().status() == TaskStatus::Running {
            (portable.poll(tick, &mut workspace), hosted.poll(tick))
        } else {
            (portable.advance(tick, &mut workspace), hosted.advance(tick))
        };
        assert_eq!(left, right);
        assert_eq!(portable.projection(), hosted.projection());
        if portable.projection().status() == TaskStatus::Completed {
            break;
        }
    }
    assert_eq!(portable.projection().status(), TaskStatus::Completed);
    let (left, resumed_runtime) = portable.into_parts();
    assert_eq!(resumed_runtime.resumed, [0, 8]);
    assert_eq!(left.records, hosted.into_parts().0.records);
}

#[test]
fn checkpoint_proposals_require_poll_storage_larger_than_final_output() {
    let mut definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    definition.budget.max_output_bytes = 8;
    let scope = scope(definition, &[1; 64]);
    let mut control = TaskControl::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    control.submit(&[1; 64], 1, TaskTick::ZERO).unwrap();
    let mut input = [0; 64];
    let mut checkpoint = [0; 64];
    let before = *control.projection();
    // A full workspace must reserve enough poll storage for a checkpoint too.
    assert_eq!(
        control.advance(
            TaskTick::new(1),
            &mut TaskWorkspace {
                input: &mut input,
                output: &mut [0; 8],
                checkpoint: &mut checkpoint,
            }
        ),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(control.projection(), &before);
    for (input_capacity, checkpoint_capacity) in [(63, 64), (64, 63)] {
        assert_eq!(
            control.advance_with_buffers(
                TaskTick::new(1),
                &mut input[..input_capacity],
                &mut checkpoint[..checkpoint_capacity]
            ),
            Err(SupervisorError::Decision(TaskError::QuotaExhausted))
        );
        assert_eq!(control.projection(), &before);
    }
    // Dispatch-only callers supply poll storage on the later poll call.
    assert_eq!(
        control.advance_with_buffers(TaskTick::new(1), &mut input, &mut checkpoint),
        Ok(true)
    );
    let running = *control.projection();
    assert_eq!(
        control.poll_with_buffer(TaskTick::new(2), &mut [0; 8]),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(control.projection(), &running);
    let mut output = [0; 64];
    for (input_capacity, checkpoint_capacity) in [(63, 64), (64, 63)] {
        assert_eq!(
            control.poll(
                TaskTick::new(2),
                &mut TaskWorkspace {
                    input: &mut input[..input_capacity],
                    output: &mut output,
                    checkpoint: &mut checkpoint[..checkpoint_capacity],
                }
            ),
            Err(SupervisorError::Decision(TaskError::QuotaExhausted))
        );
        assert_eq!(control.projection(), &running);
    }
    assert_eq!(
        control.poll_with_buffer(TaskTick::new(2), &mut output),
        Ok(true)
    );
    assert_eq!(control.projection().checkpoint().unwrap().bytes, 12);
    let (journal, runtime) = control.into_parts();
    assert_eq!(journal.records.len(), 3);
    assert_eq!(runtime.starts, 1);
}

#[test]
fn single_buffer_poll_records_timeout_without_dispatch_or_poll_storage() {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &[1]);
    let mut initial_runtime = SumRuntime::default();
    initial_runtime.fault = Some(TaskFault::Crash);
    let mut control = TaskControl::restore(
        MemoryJournal::default(),
        initial_runtime,
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    control.submit(&[1], 1, TaskTick::ZERO).unwrap();
    control
        .advance_with_buffers(TaskTick::new(1), &mut [0; 64], &mut [0; 64])
        .unwrap();
    let deadline = control.projection().execution_deadline();
    assert_eq!(control.poll_with_buffer(deadline, &mut []), Ok(true));
    assert_eq!(control.projection().status(), TaskStatus::Faulted);
    let (journal, runtime) = control.into_parts();
    assert_eq!(journal.records.len(), 3);
    assert_eq!(runtime.starts, 1);
    assert_eq!(runtime.fences, 1);
    assert_eq!(runtime.fault, Some(TaskFault::Crash));
}

#[test]
fn standalone_workspace_validation_admits_the_largest_declared_proposal() {
    let mut definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    definition.budget.max_input_bytes = 4;
    definition.budget.max_output_bytes = 16;
    definition.recovery.max_checkpoint_bytes = 12;
    let (mut input, mut output, mut checkpoint) = ([0; 4], [0; 16], [0; 12]);
    for (input_capacity, poll_capacity, checkpoint_capacity, expected) in [
        (4, 16, 12, Ok(())),
        (3, 16, 12, Err(TaskError::QuotaExhausted)),
        (4, 15, 12, Err(TaskError::QuotaExhausted)),
        (4, 16, 11, Err(TaskError::QuotaExhausted)),
    ] {
        assert_eq!(
            TaskWorkspace {
                input: &mut input[..input_capacity],
                output: &mut output[..poll_capacity],
                checkpoint: &mut checkpoint[..checkpoint_capacity],
            }
            .validate(definition),
            expected
        );
    }
    definition.budget.max_output_bytes = 8;
    for (capacity, expected) in [(12, Ok(())), (11, Err(TaskError::QuotaExhausted))] {
        assert_eq!(
            TaskWorkspace {
                input: &mut input,
                output: &mut output[..capacity],
                checkpoint: &mut checkpoint,
            }
            .validate(definition),
            expected
        );
    }
}

#[test]
fn lazy_dispatch_workspace_is_unused_for_wait_fault_and_recovery_decisions() {
    struct Workspace {
        calls: usize,
        capacity: usize,
    }
    impl TaskDispatchWorkspace for Workspace {
        fn with_buffers<T>(&mut self, operation: impl FnOnce(&mut [u8], &mut [u8]) -> T) -> T {
            self.calls += 1;
            operation(&mut [0; 64][..self.capacity], &mut [0; 64][..self.capacity])
        }
    }
    let definition = definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let scope = scope(definition, &[1]);
    let mut control = TaskControl::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    let mut workspace = Workspace {
        calls: 0,
        capacity: 0,
    };
    assert_eq!(
        control.advance_with_workspace(TaskTick::ZERO, &mut workspace),
        Ok(false)
    );
    assert_eq!(workspace.calls, 0);
    control.submit(&[1], 1, TaskTick::ZERO).unwrap();
    let pending = *control.projection();
    assert_eq!(
        control.advance_with_workspace(TaskTick::new(1), &mut workspace),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(workspace.calls, 1);
    assert_eq!(*control.projection(), pending);
    workspace.capacity = 64;
    assert_eq!(
        control.advance_with_workspace(TaskTick::new(1), &mut workspace),
        Ok(true)
    );
    assert_eq!(workspace.calls, 2);
    assert_eq!(
        control.advance_with_workspace(TaskTick::new(2), &mut workspace),
        Ok(false)
    );
    let deadline = control.projection().execution_deadline();
    assert_eq!(
        control.advance_with_workspace(deadline, &mut workspace),
        Ok(true)
    );
    assert_eq!(control.projection().status(), TaskStatus::Faulted);
    assert_eq!(
        control.advance_with_workspace(deadline, &mut workspace),
        Ok(true)
    );
    assert_eq!(workspace.calls, 2);
    let (journal, runtime) = control.into_parts();
    assert_eq!(journal.records.len(), 4);
    assert_eq!(runtime.starts, 1);
    assert_eq!(runtime.fences, 1);
}
