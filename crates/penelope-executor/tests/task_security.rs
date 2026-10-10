//! Security boundary regression tests for the reviewed task protocol.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::*;
use support::*;

#[test]
fn definition_fingerprint_pins_every_permission_and_budget_field() {
    let baseline = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let expected: Digest =
        serde_json::from_str(include_str!("fixtures/task-definition-fingerprint-v1.json")).unwrap();
    assert_eq!(
        baseline.fingerprint(),
        TaskDefinitionFingerprint::from_digest(expected)
    );
    let fixture: TaskDefinition =
        serde_json::from_str(include_str!("fixtures/task-definition-v1.json")).unwrap();
    assert_eq!(baseline, fixture);
    assert_eq!(
        serde_json::to_value(baseline).unwrap(),
        serde_json::from_str::<serde_json::Value>(include_str!("fixtures/task-definition-v1.json"))
            .unwrap()
    );
    let bound = scope(baseline, &[1; 64]);
    for index in 0..14 {
        let mut changed = baseline;
        match index {
            0 => changed.code = Digest([7; 32]),
            1 => changed.checkpoint_schema = 2,
            2 => changed.effects = EffectContract::ReconcileRequired,
            3 => changed.recovery.mode = RecoveryMode::NeverRestart,
            4 => changed.recovery.max_attempts += 1,
            5 => changed.recovery.timeout_ticks += 1,
            6 => changed.recovery.restart_delay_ticks += 1,
            7 => changed.recovery.max_checkpoint_bytes += 1,
            8 => changed.budget.max_input_bytes += 1,
            9 => changed.budget.max_output_bytes += 1,
            10 => changed.budget.max_history_records += 1,
            11 => changed.budget.max_checkpoint_commits += 1,
            12 => changed.budget.cpu_ticks += 1,
            _ => changed.budget.deadline_tick = changed.budget.deadline_tick.saturating_add(1),
        }
        assert_ne!(changed.fingerprint(), baseline.fingerprint());
        assert_eq!(
            TaskProjection::new(bound, changed),
            Err(TaskError::ScopeMismatch)
        );
    }
}

#[test]
fn task_wire_rejects_unknown_duplicate_overflow_and_wrong_shape_fields() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/task-definition-v1.json")).unwrap();
    for index in 0..5 {
        let mut wire = fixture.clone();
        match index {
            0 => wire["extra"] = true.into(),
            1 => wire["recovery"]["restart_despite_unknown"] = true.into(),
            2 => wire["budget"]["unbounded"] = true.into(),
            3 => wire["recovery"]["max_attempts"] = u64::MAX.into(),
            _ => wire["code"] = serde_json::json!([1, 2, 3]),
        }
        assert!(serde_json::from_value::<TaskDefinition>(wire).is_err());
    }
    let record: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/task-start-v1.json")).unwrap();
    let mut unknown = record.clone();
    unknown["silently_allow_restart"] = true.into();
    assert!(serde_json::from_value::<TaskRecord>(unknown).is_err());
    let mut scope = record;
    scope["scope"]["unfenced"] = true.into();
    assert!(serde_json::from_value::<TaskRecord>(scope).is_err());
    assert!(serde_json::from_str::<TaskRecord>(r#"{"version":1,"version":2}"#).is_err());
    assert!(
        serde_json::from_str::<TaskExecutionEvent>(
            r#"{"FaultObserved":{"fault":"Crash","completed":true}}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<Reconciliation>(r#"{"Unknown":{"evidence":[0],"restart":true}}"#)
            .is_err()
    );
}

#[test]
fn all_recovery_modes_preserve_ambiguity_for_interrupted_effects() {
    for mode in [
        RecoveryMode::RestartFromBeginning,
        RecoveryMode::RestartFromCheckpoint,
        RecoveryMode::ReconcileBeforeRestart,
        RecoveryMode::NeverRestart,
    ] {
        let definition = definition(mode, EffectContract::ReconcileRequired);
        let scope = scope(definition, &[1; 64]);
        let mut projection = TaskProjection::new(scope, definition).unwrap();
        let input = BlobRef {
            digest: (scope.input).digest(),
            bytes: 64,
            schema: 1,
        };
        let intent = projection
            .propose(
                TaskExecutionEvent::IntentCommitted { definition, input },
                TaskGeneration::new(0),
                TaskTick::new(0),
            )
            .unwrap();
        projection.apply(&intent).unwrap();
        let start = projection.next(TaskTick::new(1)).unwrap().unwrap();
        projection.apply(&start).unwrap();
        let fault = projection
            .propose(
                TaskExecutionEvent::FaultObserved {
                    fault: TaskFault::SupervisorInterrupted,
                },
                TaskGeneration::new(1),
                TaskTick::new(2),
            )
            .unwrap();
        projection.apply(&fault).unwrap();
        assert!(
            projection
                .propose(
                    TaskExecutionEvent::RecoveryPlanned { resume: None },
                    TaskGeneration::new(1),
                    TaskTick::new(3)
                )
                .is_err()
        );
        let reconcile = projection.next(TaskTick::new(3)).unwrap().unwrap();
        projection.apply(&reconcile).unwrap();
        let unknown = projection
            .propose(
                TaskExecutionEvent::Reconciled {
                    result: Reconciliation::Unknown {
                        evidence: Digest([7; 32]),
                    },
                },
                TaskGeneration::new(1),
                TaskTick::new(4),
            )
            .unwrap();
        projection.apply(&unknown).unwrap();
        assert!(
            projection
                .propose(
                    TaskExecutionEvent::Restarted,
                    TaskGeneration::new(2),
                    TaskTick::new(5)
                )
                .is_err()
        );
        assert!(projection.output().is_none());
        assert_eq!(projection.status(), TaskStatus::Reconciling);
    }
}
