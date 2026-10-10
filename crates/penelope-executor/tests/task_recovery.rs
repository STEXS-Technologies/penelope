//! Real adapter fault points and seeded recovery chaos, separate from saga tests.
// Reconstruction tests intentionally reuse names for fresh incarnations.
#![allow(clippy::shadow_unrelated, clippy::unreachable)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_const_for_fn,
    clippy::panic,
    clippy::needless_pass_by_value
)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::runtime::*;
use penelope_core::task::*;
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use proptest::prelude::*;
use support::*;

const INPUT: [u8; 64] = [1; 64];
type Supervisor = TaskSupervisor<MemoryJournal, SumRuntime, 64>;

fn fresh(mode: RecoveryMode, effects: EffectContract) -> Supervisor {
    let definition = definition(mode, effects);
    Supervisor::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope(definition, &INPUT),
        definition,
        0,
    )
    .unwrap()
}
fn running(mode: RecoveryMode, effects: EffectContract) -> Supervisor {
    let mut supervisor = fresh(mode, effects);
    supervisor.submit(&INPUT, 1, 0).unwrap();
    supervisor.advance(1).unwrap();
    supervisor
}
fn reconstruct(supervisor: Supervisor, tick: u64) -> Supervisor {
    let projection = *supervisor.projection();
    let (journal, runtime) = supervisor.into_parts();
    Supervisor::restore(
        journal,
        runtime,
        projection.scope(),
        projection.definition(),
        tick,
    )
    .unwrap()
}
fn finish(mut supervisor: Supervisor, mut tick: u64) -> Supervisor {
    for _step in 0..300 {
        if matches!(
            supervisor.projection().status(),
            TaskStatus::Completed | TaskStatus::Exhausted
        ) {
            return supervisor;
        }
        tick += 1;
        if supervisor.projection().status() == TaskStatus::Running {
            supervisor.poll(tick).unwrap();
        } else {
            supervisor.advance(tick).unwrap();
        }
    }
    panic!("bounded task did not terminate")
}

#[test]
fn committed_checkpoint_resumes_computation_after_supervisor_crash() {
    let mut supervisor = running(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    supervisor.poll(2).unwrap();
    let supervisor = reconstruct(supervisor, 3);
    assert_eq!(supervisor.projection().status(), TaskStatus::Faulted);
    let supervisor = finish(supervisor, 3);
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
    let output = supervisor.projection().output().unwrap();
    let (journal, runtime) = supervisor.into_parts();
    assert_eq!(runtime.resumed, [0, 8]);
    assert_eq!(journal.blobs[&output.digest.0], 64_u64.to_le_bytes());
}

#[test]
fn no_commit_or_lost_acknowledgement_never_dispatches() {
    for cut in [
        Cut::BeforeCommit,
        Cut::AfterCommit,
        Cut::WrongReceipt,
        Cut::StorageFull,
    ] {
        let definition = definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
        let journal = MemoryJournal {
            cut: Some(cut),
            ..MemoryJournal::default()
        };
        let mut supervisor = Supervisor::restore(
            journal,
            SumRuntime::default(),
            scope(definition, &INPUT),
            definition,
            0,
        )
        .unwrap();
        assert!(supervisor.submit(&INPUT, 1, 0).is_err());
        assert_eq!(
            supervisor.advance(1),
            Err(SupervisorError::ReconstructionRequired)
        );
        let (journal, runtime) = supervisor.into_parts();
        assert_eq!(runtime.starts, 0);
        assert_eq!(
            journal.records.len(),
            usize::from(matches!(cut, Cut::AfterCommit | Cut::WrongReceipt))
        );
    }
}

#[test]
fn crash_after_start_commit_before_dispatch_is_reconstructed_without_success() {
    let mut supervisor = fresh(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    supervisor.submit(&INPUT, 1, 0).unwrap();
    let projection = *supervisor.projection();
    let (mut journal, runtime) = supervisor.into_parts();
    journal.cut = Some(Cut::AfterCommit);
    let mut supervisor = Supervisor::restore(
        journal,
        runtime,
        projection.scope(),
        projection.definition(),
        1,
    )
    .unwrap();
    assert!(supervisor.advance(1).is_err());
    let (journal, runtime) = supervisor.into_parts();
    assert_eq!(runtime.starts, 0);
    let supervisor = Supervisor::restore(
        journal,
        runtime,
        projection.scope(),
        projection.definition(),
        2,
    )
    .unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Faulted);
    assert!(supervisor.projection().output().is_none());
    assert_eq!(finish(supervisor, 2).projection().generation(), 2);
}

#[test]
fn effect_interruption_requires_evidence_and_unknown_never_restarts() {
    let supervisor = running(
        RecoveryMode::RestartFromCheckpoint,
        EffectContract::ReconcileRequired,
    );
    let mut supervisor = reconstruct(supervisor, 2);
    supervisor.advance(3).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Reconciling);
    let scope = supervisor.projection().scope();
    supervisor
        .reconcile(
            scope,
            1,
            Reconciliation::Unknown {
                evidence: Digest([5; 32]),
            },
            None,
            4,
        )
        .unwrap();
    for tick in 5..20 {
        assert!(!supervisor.advance(tick).unwrap());
    }
    let (journal, runtime) = supervisor.into_parts();
    assert_eq!(runtime.starts, 1);
    let mut supervisor = Supervisor::restore(
        journal,
        runtime,
        scope,
        definition(
            RecoveryMode::RestartFromCheckpoint,
            EffectContract::ReconcileRequired,
        ),
        20,
    )
    .unwrap();
    supervisor
        .reconcile(
            scope,
            1,
            Reconciliation::RestartAuthorized {
                evidence: Digest([6; 32]),
            },
            None,
            21,
        )
        .unwrap();
    assert_eq!(
        finish(supervisor, 21).projection().status(),
        TaskStatus::Completed
    );
}

#[test]
fn independent_completion_evidence_commits_bytes_without_reexecution() {
    let supervisor = running(
        RecoveryMode::ReconcileBeforeRestart,
        EffectContract::ReconcileRequired,
    );
    let mut supervisor = reconstruct(supervisor, 2);
    supervisor.advance(3).unwrap();
    let output = 999_u64.to_le_bytes();
    let result = Reconciliation::Completed {
        output: BlobRef {
            digest: Digest::of(&output),
            bytes: 8,
            schema: 1,
        },
        evidence: Digest([8; 32]),
    };
    supervisor
        .reconcile(supervisor.projection().scope(), 1, result, Some(&output), 4)
        .unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
    assert_eq!(supervisor.into_parts().1.starts, 1);
}

#[test]
fn corrupt_checkpoint_fails_closed_before_task_code_runs() {
    let mut supervisor = running(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    supervisor.poll(2).unwrap();
    let projection = *supervisor.projection();
    let (mut journal, runtime) = supervisor.into_parts();
    journal
        .blobs
        .get_mut(&projection.checkpoint().unwrap().digest.0)
        .unwrap()[0] ^= 1;
    let mut supervisor = Supervisor::restore(
        journal,
        runtime,
        projection.scope(),
        projection.definition(),
        3,
    )
    .unwrap();
    supervisor.advance(4).unwrap();
    assert!(matches!(
        supervisor.advance(5),
        Err(SupervisorError::Decision(TaskError::CorruptBlob))
    ));
    assert_eq!(
        supervisor.advance(6),
        Err(SupervisorError::ReconstructionRequired)
    );
    assert_eq!(supervisor.into_parts().1.starts, 1);
}

#[test]
fn stale_generation_and_foreign_scope_leave_state_unchanged() {
    let supervisor = running(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let mut supervisor = reconstruct(supervisor, 2);
    supervisor.advance(3).unwrap();
    supervisor.advance(4).unwrap();
    let projection = *supervisor.projection();
    let mut observation = TaskObservation {
        scope: projection.scope(),
        generation: 1,
        progress: RuntimeProgress::Complete {
            bytes: 8,
            schema: 1,
        },
    };
    assert!(matches!(
        supervisor.observe(observation, &[0; 8], 5),
        Err(SupervisorError::Decision(TaskError::StaleGeneration))
    ));
    observation.generation = 2;
    observation.scope.tenant = Digest([9; 32]);
    assert!(matches!(
        supervisor.observe(observation, &[0; 8], 5),
        Err(SupervisorError::Decision(TaskError::ScopeMismatch))
    ));
    assert_eq!(*supervisor.projection(), projection);
}

#[test]
fn checkpoint_commit_ack_loss_is_recovered_from_committed_boundary() {
    let supervisor = running(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let projection = *supervisor.projection();
    let (mut journal, runtime) = supervisor.into_parts();
    let checkpoint = [8, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0];
    let record = projection
        .propose(
            TaskExecutionEvent::CheckpointCommitted {
                checkpoint: BlobRef {
                    digest: Digest::of(&checkpoint),
                    bytes: 12,
                    schema: 1,
                },
            },
            1,
            2,
        )
        .unwrap();
    journal.cut = Some(Cut::AfterCommit);
    assert_eq!(
        journal.commit(&record, Some(&checkpoint)),
        Err(TaskPortError::Unavailable)
    );
    let supervisor = Supervisor::restore(
        journal,
        runtime,
        projection.scope(),
        projection.definition(),
        3,
    )
    .unwrap();
    assert_eq!(finish(supervisor, 3).into_parts().1.resumed, [0, 8]);
}

#[test]
fn failed_runtime_checkpoint_acknowledgement_requires_replay_of_committed_checkpoint() {
    let supervisor = fresh(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let projection = *supervisor.projection();
    let (journal, mut runtime) = supervisor.into_parts();
    runtime.acknowledge_failure = true;
    let mut supervisor = Supervisor::restore(
        journal,
        runtime,
        projection.scope(),
        projection.definition(),
        0,
    )
    .unwrap();
    supervisor.submit(&INPUT, 1, 0).unwrap();
    supervisor.advance(1).unwrap();
    assert!(matches!(
        supervisor.poll(2),
        Err(SupervisorError::Port(TaskPortError::Runtime))
    ));
    assert!(supervisor.projection().checkpoint().is_some());
    assert_eq!(
        supervisor.poll(3),
        Err(SupervisorError::ReconstructionRequired)
    );
    let (journal, mut runtime) = supervisor.into_parts();
    runtime.acknowledge_failure = false;
    let recovered = Supervisor::restore(
        journal,
        runtime,
        projection.scope(),
        projection.definition(),
        3,
    )
    .unwrap();
    assert_eq!(finish(recovered, 3).into_parts().1.resumed, [0, 8]);
}

#[test]
fn regressed_clock_is_rejected_before_runtime_poll_consumes_progress() {
    let mut supervisor = running(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    supervisor.poll(2).unwrap();
    let before = *supervisor.projection();
    assert_eq!(
        supervisor.poll(1),
        Err(SupervisorError::Decision(TaskError::InvalidOrder))
    );
    assert_eq!(*supervisor.projection(), before);
    assert!(supervisor.poll(3).unwrap());
    assert_eq!(
        finish(supervisor, 3).projection().status(),
        TaskStatus::Completed
    );
}

#[test]
fn oversized_input_migration_and_reconciled_output_are_rejected_before_commit() {
    let mut supervisor = fresh(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    assert_eq!(
        supervisor.submit(&[1; 65], 1, 0),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(supervisor.projection().next_sequence(), 0);
    supervisor.submit(&INPUT, 1, 0).unwrap();
    let source = TaskScope {
        definition: Digest([9; 32]),
        ..supervisor.projection().scope()
    };
    let previous = BlobRef {
        digest: Digest([8; 32]),
        bytes: 12,
        schema: 1,
    };
    assert_eq!(
        supervisor.import_checkpoint(source, previous, &[0; 65], Digest([7; 32]), 1),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(supervisor.projection().next_sequence(), 1);
    let mut external = reconstruct(
        running(
            RecoveryMode::ReconcileBeforeRestart,
            EffectContract::ReconcileRequired,
        ),
        2,
    );
    external.advance(3).unwrap();
    let output = BlobRef {
        digest: Digest::of(&[1; 65]),
        bytes: 65,
        schema: 1,
    };
    assert_eq!(
        external.reconcile(
            external.projection().scope(),
            1,
            Reconciliation::Completed {
                output,
                evidence: Digest([5; 32])
            },
            Some(&[1; 65]),
            4
        ),
        Err(SupervisorError::Decision(TaskError::QuotaExhausted))
    );
    assert_eq!(external.projection().status(), TaskStatus::Reconciling);
}

#[test]
fn timeout_late_results_and_fence_failure_do_not_claim_success() {
    let mut supervisor = running(RecoveryMode::NeverRestart, EffectContract::Pure);
    assert!(matches!(
        supervisor.observe(
            TaskObservation {
                scope: supervisor.projection().scope(),
                generation: 1,
                progress: RuntimeProgress::Complete {
                    bytes: 8,
                    schema: 1
                }
            },
            &[0; 8],
            51
        ),
        Err(SupervisorError::Decision(TaskError::DeadlineExpired))
    ));
    supervisor.poll(51).unwrap();
    supervisor.advance(52).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Exhausted);
    let supervisor = running(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let projection = *supervisor.projection();
    let (journal, mut runtime) = supervisor.into_parts();
    runtime.fence_failure = true;
    assert!(matches!(
        Supervisor::restore(
            journal,
            runtime,
            projection.scope(),
            projection.definition(),
            2
        ),
        Err(SupervisorError::Port(TaskPortError::Conflict))
    ));
}

#[test]
fn repeated_faults_are_bounded_by_generation_quota() {
    let mut supervisor = running(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let mut tick = 1;
    for generation in 1..=32 {
        tick += 1;
        supervisor
            .observe(
                TaskObservation {
                    scope: supervisor.projection().scope(),
                    generation,
                    progress: RuntimeProgress::Fault(TaskFault::Crash),
                },
                &[],
                tick,
            )
            .unwrap();
        tick += 1;
        supervisor.advance(tick).unwrap();
        if generation < 32 {
            tick += 1;
            supervisor.advance(tick).unwrap();
        }
    }
    assert_eq!(supervisor.projection().status(), TaskStatus::Exhausted);
    assert_eq!(supervisor.into_parts().1.starts, 32);
}

proptest! {
    #[test]
    fn seeded_checkpoint_crash_and_persistence_chaos(cuts in prop::collection::vec(0_u8..5, 0..20)) {
        let mut supervisor = running(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
        let mut tick = 1_u64;
        for cut in cuts {
            tick += 1;
            let projection = *supervisor.projection();
            let (mut journal, mut runtime) = supervisor.into_parts();
            journal.cut = match cut { 0 => Some(Cut::BeforeCommit), 1 => Some(Cut::AfterCommit), 2 => Some(Cut::WrongReceipt), 3 => Some(Cut::StorageFull), _ => None };
            // Reconstruct with interruption acknowledgement uninjected, then
            // inject the cut into the checkpoint/next transition itself.
            let injected = journal.cut.take();
            supervisor = Supervisor::restore(journal, runtime, projection.scope(), projection.definition(), tick).unwrap();
            tick += 1; supervisor.advance(tick).unwrap();
            tick += 1; supervisor.advance(tick).unwrap();
            let projection = *supervisor.projection();
            let (mut journal, runtime_adapter) = supervisor.into_parts();
            runtime = runtime_adapter;
            journal.cut = injected;
            // Construct from replay without treating the live adapter as a crash.
            let mut buffer = [0; 64];
            let observation = runtime.poll(projection.scope(), projection.generation(), &mut buffer).unwrap();
            let (reference, event) = match observation.progress {
                RuntimeProgress::Checkpoint { bytes, schema } => {
                    let reference = BlobRef { digest: Digest::of(&buffer[..bytes as usize]), bytes, schema };
                    (reference, TaskExecutionEvent::CheckpointCommitted { checkpoint: reference })
                }
                RuntimeProgress::Complete { bytes, schema } => {
                    let reference = BlobRef { digest: Digest::of(&buffer[..bytes as usize]), bytes, schema };
                    (reference, TaskExecutionEvent::Completed { output: reference })
                }
                RuntimeProgress::Pending | RuntimeProgress::Fault(_) => unreachable!(),
            };
            tick += 1;
            let record = projection.propose(event, projection.generation(), tick).unwrap();
            let _commit = journal.commit(&record, Some(&buffer[..reference.bytes as usize]));
            tick += 1;
            supervisor = Supervisor::restore(journal, runtime, projection.scope(), projection.definition(), tick).unwrap();
            if supervisor.projection().status() == TaskStatus::Completed { break; }
        }
        let supervisor = finish(supervisor, tick);
        prop_assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
        let output = supervisor.projection().output().unwrap();
        let (journal, runtime) = supervisor.into_parts();
        prop_assert_eq!(&journal.blobs[&output.digest.0], &64_u64.to_le_bytes());
        prop_assert!(runtime.starts <= 32);
    }
}
