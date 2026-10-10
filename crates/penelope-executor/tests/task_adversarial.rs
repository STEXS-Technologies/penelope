//! Policy and persistence adversaries that must never authorize unsafe progress.
// Test adapters deliberately inject faults after normal construction.
#![allow(clippy::field_reassign_with_default)]
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::shadow_unrelated
)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::runtime::*;
use penelope_core::task::*;
use penelope_executor::task::{SupervisorError, TaskSupervisor};
use support::*;

const INPUT: [u8; 64] = [1; 64];
type Supervisor = TaskSupervisor<MemoryJournal, SumRuntime, 64>;
fn intent(definition: TaskDefinition) -> TaskRecord {
    let scope = scope(definition, &INPUT);
    TaskProjection::new(scope, definition)
        .unwrap()
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition,
                input: BlobRef {
                    digest: scope.input,
                    bytes: 64,
                    schema: 1,
                },
            },
            0,
            0,
        )
        .unwrap()
}
fn running(definition: TaskDefinition) -> TaskProjection {
    let initial = intent(definition);
    let mut projection = replay(initial.scope, definition, [initial]).unwrap();
    let start = projection.next(1).unwrap().unwrap();
    projection.apply(&start).unwrap();
    projection
}

#[test]
fn invalid_definitions_and_incompatible_intents_never_become_pending() {
    let valid = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    for index in 0..6 {
        let mut bad = valid;
        match index {
            0 => bad.recovery.max_attempts = 0,
            1 => bad.recovery.timeout_ticks = 0,
            2 => bad.budget.cpu_ticks = 0,
            3 => bad.budget.max_history_records = 3,
            4 => bad.budget.deadline_tick = 0,
            _ => bad.checkpoint_schema = 0,
        }
        assert_eq!(bad.validate(), Err(TaskError::InvalidDefinition));
        assert_eq!(
            TaskProjection::new(scope(bad, &INPUT), bad),
            Err(TaskError::InvalidDefinition)
        );
    }
    let initial = intent(valid);
    for index in 0..3 {
        let mut bad = initial;
        if let TaskExecutionEvent::IntentCommitted {
            ref mut definition,
            ref mut input,
        } = bad.event
        {
            match index {
                0 => definition.recovery.max_attempts = 1,
                1 => input.digest = Digest([9; 32]),
                _ => input.bytes = 65,
            }
        }
        assert!(replay(initial.scope, valid, [bad]).is_err());
    }
    let reference = BlobRef {
        digest: Digest::of(&INPUT),
        bytes: 64,
        schema: 1,
    };
    assert_eq!(reference.verify(&INPUT, 2), Err(TaskError::SchemaMismatch));
}

#[test]
fn valid_replay_survives_every_prefix_and_corrupt_history_is_rejected() {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let initial = intent(definition);
    let mut records = vec![initial];
    let mut live = replay(initial.scope, definition, records.clone()).unwrap();
    for tick in 1..5 {
        let event = match tick {
            1 => TaskExecutionEvent::Started,
            2 => TaskExecutionEvent::CheckpointCommitted {
                checkpoint: BlobRef {
                    digest: Digest([7; 32]),
                    bytes: 12,
                    schema: 1,
                },
            },
            3 => TaskExecutionEvent::FaultObserved {
                fault: TaskFault::Crash,
            },
            _ => TaskExecutionEvent::RecoveryPlanned {
                resume: live.checkpoint(),
            },
        };
        let record = live.propose(event, 1, tick).unwrap();
        live.apply(&record).unwrap();
        records.push(record);
        assert_eq!(
            replay(initial.scope, definition, records.clone()).unwrap(),
            live
        );
    }
    let mut bad_history = records;
    bad_history.last_mut().unwrap().sequence = 90;
    assert_eq!(
        replay(initial.scope, definition, bad_history),
        Err(TaskError::InvalidOrder)
    );
}

#[test]
fn cooldown_cannot_be_bypassed_and_checkpoint_substitution_cannot_authorize_restart() {
    let mut definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    definition.recovery.restart_delay_ticks = 10;
    let mut projection = running(definition);
    assert_eq!(projection.next(2).unwrap(), None);
    let fault = projection
        .propose(
            TaskExecutionEvent::FaultObserved {
                fault: TaskFault::Crash,
            },
            1,
            3,
        )
        .unwrap();
    projection.apply(&fault).unwrap();
    assert_eq!(
        projection.propose(TaskExecutionEvent::RecoveryExhausted, 1, 4),
        Err(TaskError::InvalidTransition)
    );
    assert_eq!(
        projection.propose(
            TaskExecutionEvent::RecoveryPlanned {
                resume: Some(BlobRef {
                    digest: Digest([9; 32]),
                    bytes: 12,
                    schema: 1
                })
            },
            1,
            4
        ),
        Err(TaskError::CorruptBlob)
    );
    let planned = projection.next(4).unwrap().unwrap();
    projection.apply(&planned).unwrap();
    assert_eq!(projection.next(5).unwrap(), None);
    assert_eq!(
        projection.propose(TaskExecutionEvent::Restarted, 2, 5),
        Err(TaskError::InvalidTransition)
    );
    let restart = projection.next(13).unwrap().unwrap();
    projection.apply(&restart).unwrap();
    assert_eq!(projection.generation(), 2);
}

#[test]
fn history_and_attempt_quotas_fail_closed_before_new_authorization() {
    let mut definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    definition.budget.max_history_records = 4;
    let mut projection = running(definition);
    let checkpoint = projection
        .propose(
            TaskExecutionEvent::CheckpointCommitted {
                checkpoint: BlobRef {
                    digest: Digest([7; 32]),
                    bytes: 12,
                    schema: 1,
                },
            },
            1,
            2,
        )
        .unwrap();
    projection.apply(&checkpoint).unwrap();
    let fault = projection
        .propose(
            TaskExecutionEvent::FaultObserved {
                fault: TaskFault::Crash,
            },
            1,
            3,
        )
        .unwrap();
    projection.apply(&fault).unwrap();
    assert_eq!(projection.next(4), Err(TaskError::QuotaExhausted));
    assert_eq!(projection.status(), TaskStatus::Faulted);
    definition.budget.max_history_records = 50;
    definition.recovery.max_attempts = 1;
    let mut projection = running(definition);
    let fault = projection
        .propose(
            TaskExecutionEvent::FaultObserved {
                fault: TaskFault::Crash,
            },
            1,
            2,
        )
        .unwrap();
    projection.apply(&fault).unwrap();
    assert_eq!(
        projection.propose(TaskExecutionEvent::RecoveryPlanned { resume: None }, 1, 3),
        Err(TaskError::QuotaExhausted)
    );
}

#[test]
fn premature_faults_completion_and_reconciliation_are_rejected() {
    let projection = running(definition(
        RecoveryMode::RestartFromBeginning,
        EffectContract::Pure,
    ));
    for fault in [TaskFault::Timeout, TaskFault::DeadlineViolation] {
        assert_eq!(
            projection.propose(TaskExecutionEvent::FaultObserved { fault }, 1, 2),
            Err(TaskError::InvalidTransition)
        );
    }
    assert_eq!(
        projection.propose(TaskExecutionEvent::RecoveryExhausted, 1, 2),
        Err(TaskError::InvalidTransition)
    );
    assert_eq!(
        projection.propose(
            TaskExecutionEvent::Completed {
                output: BlobRef {
                    digest: Digest([6; 32]),
                    bytes: 65,
                    schema: 1
                }
            },
            1,
            2
        ),
        Err(TaskError::QuotaExhausted)
    );
    let mut faulted = projection;
    let fault = faulted
        .propose(
            TaskExecutionEvent::FaultObserved {
                fault: TaskFault::Crash,
            },
            1,
            2,
        )
        .unwrap();
    faulted.apply(&fault).unwrap();
    assert_eq!(
        faulted.propose(TaskExecutionEvent::ReconciliationRequired, 1, 3),
        Err(TaskError::InvalidTransition)
    );
}

#[test]
fn migration_rejects_foreign_task_wrong_schema_and_oversized_checkpoint() {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let initial = intent(definition);
    let projection = replay(initial.scope, definition, [initial]).unwrap();
    let previous = BlobRef {
        digest: Digest([7; 32]),
        bytes: 12,
        schema: 1,
    };
    for index in 0..3 {
        let mut source = TaskScope {
            definition: Digest([8; 32]),
            ..projection.scope()
        };
        let mut checkpoint = previous;
        match index {
            0 => source.task = Digest([9; 32]),
            1 => checkpoint.schema = 2,
            _ => checkpoint.bytes = 65,
        }
        assert!(
            projection
                .propose(
                    TaskExecutionEvent::CheckpointMigrated {
                        source,
                        previous,
                        checkpoint,
                        authorization: Digest([5; 32])
                    },
                    0,
                    1
                )
                .is_err()
        );
    }
}

#[test]
fn poisoned_restore_corrupt_journal_and_runtime_failure_never_claim_completion() {
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let initial = intent(definition);
    let scope = initial.scope;
    let mut supervisor = Supervisor::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        0,
    )
    .unwrap();
    supervisor.submit(&INPUT, 1, 0).unwrap();
    supervisor.advance(1).unwrap();
    let (journal, runtime) = supervisor.into_parts();
    assert!(matches!(
        Supervisor::restore(journal.clone(), runtime.clone(), scope, definition, 0),
        Err(SupervisorError::Decision(TaskError::InvalidOrder))
    ));
    let mut corrupt = journal.clone();
    corrupt.records[0].version = 9;
    assert!(matches!(
        Supervisor::restore(corrupt, runtime.clone(), scope, definition, 2),
        Err(SupervisorError::Decision(TaskError::UnsupportedVersion))
    ));
    for cut in [Cut::BeforeCommit, Cut::WrongReceipt] {
        let mut injected = journal.clone();
        injected.cut = Some(cut);
        assert!(Supervisor::restore(injected, runtime.clone(), scope, definition, 2).is_err());
    }
    let mut failed_runtime = SumRuntime::default();
    failed_runtime.fence_failure = true;
    // Live fence failure is different from a failing restore: no fault record commits.
    let mut valid = Supervisor::restore(
        MemoryJournal::default(),
        failed_runtime,
        scope,
        definition,
        0,
    )
    .unwrap();
    valid.submit(&INPUT, 1, 0).unwrap();
    valid.advance(1).unwrap();
    valid.advance(51).unwrap_err();
    assert_eq!(valid.projection().status(), TaskStatus::Running);
    assert_eq!(valid.projection().next_sequence(), 2);
    assert_eq!(
        valid.advance(52),
        Err(SupervisorError::ReconstructionRequired)
    );
}

#[test]
fn pending_observations_and_wrong_reconciliation_evidence_do_not_advance_history() {
    let definition = definition(
        RecoveryMode::ReconcileBeforeRestart,
        EffectContract::ReconcileRequired,
    );
    let scope = scope(definition, &INPUT);
    let mut supervisor = Supervisor::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        0,
    )
    .unwrap();
    assert!(supervisor.poll(0).is_err());
    supervisor.submit(&INPUT, 1, 0).unwrap();
    supervisor.advance(1).unwrap();
    assert!(
        !supervisor
            .observe(
                TaskObservation {
                    scope,
                    generation: 1,
                    progress: RuntimeProgress::Pending
                },
                &[],
                2
            )
            .unwrap()
    );
    supervisor
        .observe(
            TaskObservation {
                scope,
                generation: 1,
                progress: RuntimeProgress::Fault(TaskFault::Crash),
            },
            &[],
            2,
        )
        .unwrap();
    supervisor.advance(3).unwrap();
    let evidence = Reconciliation::RestartAuthorized {
        evidence: Digest([7; 32]),
    };
    let foreign = TaskScope {
        task: Digest([9; 32]),
        ..scope
    };
    assert_eq!(
        supervisor.reconcile(foreign, 1, evidence, None, 4),
        Err(SupervisorError::Decision(TaskError::ScopeMismatch))
    );
    assert_eq!(
        supervisor.reconcile(scope, 0, evidence, None, 4),
        Err(SupervisorError::Decision(TaskError::StaleGeneration))
    );
    assert_eq!(
        supervisor.reconcile(scope, 1, evidence, Some(&[]), 4),
        Err(SupervisorError::Decision(TaskError::CorruptBlob))
    );
    assert_eq!(supervisor.projection().status(), TaskStatus::Reconciling);
    assert!(
        supervisor
            .observe(
                TaskObservation {
                    scope,
                    generation: 1,
                    progress: RuntimeProgress::Pending
                },
                &[],
                4
            )
            .is_err()
    );
}
