//! Prototype a pure recoverable task through existing saga effect contracts.
#![allow(clippy::unwrap_used, clippy::missing_const_for_fn)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::*;
use penelope_domain::{ContentDigest, ProcessAction, ProcessActionKind, ProcessScope};
use penelope_executor::task::TaskSupervisor;
use penelope_ports::{
    EffectDispatchRequest, ExternalEffectState,
    tasks::{task_effect_evidence, task_scope_for_effect},
};
use support::{MemoryJournal, SumRuntime, definition};

#[test]
fn existing_effect_identity_survives_task_crash_without_changing_saga_records() {
    let process_scope = ProcessScope::new(
        "tnt_demo".try_into().unwrap(),
        "prc_demo".try_into().unwrap(),
        "def_demo".try_into().unwrap(),
        "dfv_one".try_into().unwrap(),
        ContentDigest([1; 32]),
    );
    let action = ProcessAction::new(
        process_scope,
        "act_compute".try_into().unwrap(),
        "stp_compute".try_into().unwrap(),
        0,
        ProcessActionKind::ExternalEffect,
        ContentDigest::sha256(&[1; 64]),
    );
    let request = EffectDispatchRequest::new(action);
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = task_scope_for_effect(&request, definition).unwrap();
    let mut task = TaskSupervisor::<_, _, 64>::restore(
        MemoryJournal::default(),
        SumRuntime::default(),
        scope,
        definition,
        0,
    )
    .unwrap();
    task.submit(&[1; 64], 1, 0).unwrap();
    task.advance(1).unwrap();
    task.poll(2).unwrap();
    assert_eq!(
        task_effect_evidence(&request, task.projection())
            .unwrap()
            .state,
        ExternalEffectState::Unknown
    );
    let (journal, runtime) = task.into_parts();
    let mut recovered =
        TaskSupervisor::<_, _, 64>::restore(journal, runtime, scope, definition, 3).unwrap();
    for tick in 4..30 {
        if recovered.projection().status() == TaskStatus::Completed {
            break;
        }
        if recovered.projection().status() == TaskStatus::Running {
            recovered.poll(tick).unwrap();
        } else {
            recovered.advance(tick).unwrap();
        }
    }
    let evidence = task_effect_evidence(&request, recovered.projection()).unwrap();
    assert_eq!(evidence.state, ExternalEffectState::Succeeded);
    assert_eq!(evidence.effect_key, request.effect_key);
    assert_eq!(evidence.action_id, request.action.action_id);
    evidence.validate_for(&request).unwrap();
    assert_eq!(recovered.projection().generation(), 2);
    assert_eq!(recovered.into_parts().1.resumed, [0, 8]);
    let mut substituted = request;
    substituted.action.payload_digest = ContentDigest([9; 32]);
    assert!(task_scope_for_effect(&substituted, definition).is_err());
}
