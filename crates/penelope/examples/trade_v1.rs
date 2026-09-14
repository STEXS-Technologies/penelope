//! A pure `trade.v1` happy-path simulation.
//!
//! The integration layer must first build and durably record the outcome plan
//! for each event/decision, then submit the returned action with its stable
//! action ID. It may advance only after verified, correlated evidence arrives.

use penelope::{
    ActionId, ActionResultObservationV1, CausationIdV1, ContentDigest, DefinitionId,
    DefinitionVersion, DomainError, InputId, LinearSagaDefinitionV1, LinearSagaEventV1,
    LogicalTimeV1, OutcomeActorV1, OutcomeId, ProcessId, ProcessOutcomeDtoV1, ProcessOutcomeFactV1,
    ProcessScopeV1, RetryPolicyV1, SagaDecisionV1, SagaStatusV1, StepId, StepPlanV1, TenantId,
    apply_action_result, start,
};
use thiserror::Error;

#[derive(Debug, Error)]
enum ExampleError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Engine(#[from] penelope::EngineError),
    #[error("trade simulation did not complete")]
    NotCompleted,
    #[error("trade simulation ran out of preallocated outcome identities")]
    MissingOutcomeId,
    #[error("trade outcome sequence overflowed")]
    OutcomeSequenceOverflow,
}

fn identifier<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn causation_for(event: &LinearSagaEventV1) -> Result<CausationIdV1, ExampleError> {
    Ok(match event {
        LinearSagaEventV1::Started { action_id, .. } => CausationIdV1::Action(action_id.clone()),
        LinearSagaEventV1::ActionResultObserved { observation, .. }
        | LinearSagaEventV1::RetryTimerScheduled { observation, .. } => {
            CausationIdV1::Action(observation.action_id.clone())
        }
        LinearSagaEventV1::RetryTimerFired {
            timer_action_id, ..
        } => CausationIdV1::Action(timer_action_id.clone()),
        LinearSagaEventV1::ManualResolutionApplied { .. } => {
            CausationIdV1::Input(identifier::<InputId>("inp_manual_resolution")?)
        }
    })
}

fn validate_outcome_plan(
    event: &LinearSagaEventV1,
    decision: &SagaDecisionV1,
    scope: &ProcessScopeV1,
    next_sequence: &mut u64,
    outcome_ids: &mut impl Iterator<Item = OutcomeId>,
) -> Result<(), ExampleError> {
    let causation_id = causation_for(event)?;
    let mut outcomes = Vec::new();
    for kind in event
        .observed_outcome_kinds()
        .iter()
        .chain(decision.planned_outcome_kinds())
    {
        let outcome_id = outcome_ids.next().ok_or(ExampleError::MissingOutcomeId)?;
        outcomes.push(ProcessOutcomeDtoV1::new(
            scope.clone(),
            *next_sequence,
            ProcessOutcomeFactV1::new(
                outcome_id,
                causation_id.clone(),
                OutcomeActorV1::System,
                LogicalTimeV1(*next_sequence),
                *kind,
                ContentDigest([0; 32]),
            ),
        ));
        *next_sequence = next_sequence
            .checked_add(1)
            .ok_or(ExampleError::OutcomeSequenceOverflow)?;
    }
    decision.validate_required_outcomes(event, &outcomes)?;
    Ok(())
}

fn main() -> Result<(), ExampleError> {
    let definition = LinearSagaDefinitionV1::new(
        identifier::<DefinitionId>("def_trade")?,
        identifier::<DefinitionVersion>("dfv_one")?,
        ContentDigest([99; 32]),
        vec![
            StepPlanV1::canonical_command(
                identifier::<StepId>("stp_lock_seller")?,
                ContentDigest([1; 32]),
                RetryPolicyV1::no_retry(),
            ),
            StepPlanV1::canonical_command(
                identifier::<StepId>("stp_lock_buyer")?,
                ContentDigest([2; 32]),
                RetryPolicyV1::no_retry(),
            ),
            StepPlanV1::canonical_command(
                identifier::<StepId>("stp_settle")?,
                ContentDigest([3; 32]),
                RetryPolicyV1::no_retry(),
            ),
        ],
    );
    let tenant_id = identifier::<TenantId>("tnt_market")?;
    let process_id = identifier::<ProcessId>("prc_trade_42")?;
    let scope = ProcessScopeV1::new(
        tenant_id.clone(),
        process_id.clone(),
        definition.definition_id.clone(),
        definition.definition_version.clone(),
        definition.definition_digest,
    );
    let mut next_action_ids = [
        identifier::<ActionId>("act_lock_buyer")?,
        identifier::<ActionId>("act_settle")?,
    ]
    .into_iter();
    let mut outcome_ids = [
        identifier::<OutcomeId>("out_trade_started")?,
        identifier("out_trade_lock_seller_planned")?,
        identifier("out_trade_lock_seller_succeeded")?,
        identifier("out_trade_lock_buyer_planned")?,
        identifier("out_trade_lock_buyer_succeeded")?,
        identifier("out_trade_settle_planned")?,
        identifier("out_trade_settle_succeeded")?,
        identifier("out_trade_completed")?,
    ]
    .into_iter();
    let mut next_sequence = 0_u64;
    let first_action_id = identifier::<ActionId>("act_lock_seller")?;
    let start_event = LinearSagaEventV1::started(&definition, first_action_id.clone());
    let mut decision = start(
        &definition,
        tenant_id.clone(),
        process_id.clone(),
        first_action_id,
    )?;
    validate_outcome_plan(
        &start_event,
        &decision,
        &scope,
        &mut next_sequence,
        &mut outcome_ids,
    )?;

    while let Some(action) = decision.next_action.as_ref() {
        // A real adapter obtains this only after durable StateChronicle evidence.
        let observed = ActionResultObservationV1::succeeded(action.action_id.clone());
        let next_action_id = next_action_ids.next();
        let event = LinearSagaEventV1::ActionResultObserved {
            observation: observed.clone(),
            next_action_id: next_action_id.clone(),
        };
        decision = apply_action_result(
            &definition,
            &decision.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observed,
            next_action_id,
        )?;
        validate_outcome_plan(
            &event,
            &decision,
            &scope,
            &mut next_sequence,
            &mut outcome_ids,
        )?;
    }

    if decision.projection.status == SagaStatusV1::Completed {
        Ok(())
    } else {
        Err(ExampleError::NotCompleted)
    }
}
