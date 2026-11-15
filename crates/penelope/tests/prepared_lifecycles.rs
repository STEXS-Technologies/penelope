//! Public proposals preserve native authority and never mutate their source.
#![allow(clippy::panic_in_result_fn)]
use penelope::{
    ContentDigest, ProcessId, TenantId, diagnostics::PreparedDecision, saga::*, task::*,
};
use std::{error::Error, num::NonZeroU32};
fn definition() -> Result<LinearSagaDefinition, Box<dyn Error>> {
    Ok(LinearSagaDefinition::new(
        "def_public".parse()?,
        "dfv_one".parse()?,
        ContentDigest([1; 32]),
        vec![StepPlan::canonical_command(
            "stp_work".parse()?,
            ContentDigest([2; 32]),
            RetryPolicy::no_retry(),
        )],
    ))
}
#[test]
fn linear_proposals_preserve_unknown_outcomes_and_require_separate_admission()
-> Result<(), Box<dyn Error>> {
    let definition = definition()?;
    let tenant: TenantId = "tnt_public".parse()?;
    let process: ProcessId = "prc_public".parse()?;
    let input = LinearSagaInput::Start {
        input_id: "inp_start".parse()?,
        action_id: "act_work".parse()?,
    };
    let ready = prepare_linear(&definition, None, &tenant, &process, &input)?;
    assert!(ready.before().is_none());
    assert_eq!(ready.candidate().status, SagaStatus::Running);
    assert_eq!(ready.provenance().tenant, &tenant);
    assert_eq!(ready.provenance().process, &process);
    assert_eq!(ready.provenance().input, &input);
    assert_eq!(ready.intents().count(), 1);
    assert_eq!(
        ready
            .decision()
            .next_action
            .as_ref()
            .map(|a| a.action_id.clone()),
        Some("act_work".parse()?)
    );
    let (event, admitted) = ready.into_parts();
    assert_eq!(event, input.to_event(&definition));
    let before = admitted.projection;
    let result = LinearSagaInput::ActionResult {
        input_id: "inp_unknown".parse()?,
        observation: ActionResultObservation {
            action_id: "act_work".parse()?,
            result: ActionResult::Unknown,
        },
        next_action_id: None,
    };
    let result_ready = prepare_linear(&definition, Some(&before), &tenant, &process, &result)?;
    assert_eq!(result_ready.before(), Some(&before));
    assert_eq!(result_ready.candidate().status, SagaStatus::Escalated);
    assert_eq!(result_ready.intents().count(), 0);
    drop(result_ready);
    assert_eq!(before.status, SagaStatus::Running);
    let mut invalid = definition.clone();
    invalid.steps.clear();
    assert!(matches!(
        prepare_linear(&invalid, Some(&before), &tenant, &process, &result),
        Err(EngineError::EmptyDefinition)
    ));
    Ok(())
}
#[test]
fn graph_preparation_retains_scope_and_refuses_foreign_action_without_mutation()
-> Result<(), Box<dyn Error>> {
    let linear = definition()?;
    let definition = ProcessGraphDefinition {
        definition_id: linear.definition_id,
        definition_version: linear.definition_version,
        definition_digest: linear.definition_digest,
        steps: linear.steps,
        entry_step_id: "stp_work".parse()?,
        transitions: vec![GraphTransition {
            from_step: "stp_work".parse()?,
            on: GraphTransitionOutcome::Succeeded,
            to_step: None,
        }],
        max_step_visits: NonZeroU32::new(1).ok_or("invalid visits")?,
    };
    let ready = prepare_graph_start(
        &definition,
        "tnt_public".parse()?,
        "prc_public".parse()?,
        GraphSagaInput::Start {
            input_id: "inp_start".parse()?,
            action_id: "act_work".parse()?,
        },
    )?;
    assert!(ready.before().is_none());
    assert_eq!(ready.intents().count(), 1);
    assert_eq!(ready.provenance(), &ready.decision().event);
    assert_eq!(ready.candidate().scope.tenant_id.as_str(), "tnt_public");
    let admitted = ready.into_decision();
    let before = admitted.projection;
    let mut input = GraphSagaInput::ActionResult {
        input_id: "inp_result".parse()?,
        observation: ActionResultObservation {
            action_id: "act_foreign".parse()?,
            result: ActionResult::Succeeded,
        },
        next_action_id: None,
    };
    assert!(matches!(
        prepare_graph_result(&definition, &before, input.clone()),
        Err(GraphEngineError::ActionMismatch)
    ));
    if let GraphSagaInput::ActionResult { observation, .. } = &mut input {
        observation.action_id = "act_work".parse()?;
    }
    let result_ready = prepare_graph_result(&definition, &before, input)?;
    assert_eq!(result_ready.before(), Some(&before));
    assert_eq!(result_ready.candidate().status, SagaStatus::Completed);
    assert_eq!(result_ready.intents().count(), 0);
    assert_eq!(result_ready.provenance(), &result_ready.decision().event);
    let completed = result_ready.into_decision();
    assert_eq!(completed.projection.scope, before.scope);
    assert_eq!(before.status, SagaStatus::Running);
    Ok(())
}
#[test]
fn task_prepared_history_is_not_applied_until_the_native_owner_admits_it()
-> Result<(), Box<dyn Error>> {
    let definition = TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::NeverRestart,
            max_attempts: 1,
            timeout_ticks: 10,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 8,
        },
        budget: ExecutionBudget {
            max_input_bytes: 8,
            max_output_bytes: 8,
            max_history_records: 8,
            max_checkpoint_commits: 0,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(100),
        },
    };
    let input = BlobRef {
        digest: Digest::of(&[1; 8]),
        bytes: 8,
        schema: 1,
    };
    let scope = TaskScope {
        tenant: TaskTenantId::from_digest(Digest([4; 32])),
        task: TaskId::from_digest(Digest([5; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(input.digest),
    };
    let projection = TaskProjection::new(scope, definition)?;
    let record = projection.propose(
        TaskExecutionEvent::IntentCommitted { definition, input },
        TaskGeneration::ZERO,
        TaskTick::ZERO,
    )?;
    let ready = projection.prepare(record)?;
    assert_eq!(ready.before(), Some(&projection));
    assert_eq!(ready.candidate().status(), TaskStatus::Pending);
    assert_eq!(ready.provenance(), &record);
    assert_eq!(ready.intents().count(), 0);
    assert_eq!(ready.into_record(), record);
    assert_eq!(projection.status(), TaskStatus::Empty);
    let mut invalid = record;
    invalid.generation = TaskGeneration::new(7);
    assert!(matches!(
        projection.prepare(invalid),
        Err(TaskError::StaleGeneration)
    ));
    Ok(())
}
