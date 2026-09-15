//! A pure `trade.v1` settlement-unknown and authorized-resolution simulation.
//!
//! The outer application must verify and durably record the manual-review
//! decision before giving this typed resolution to the pure engine.

use penelope::{
    ActionId, ActionResultObservation, CompensationPlan, ContentDigest, DefinitionId,
    DefinitionVersion, DomainError, EngineError, LinearSagaDefinition, ManualReviewResolution,
    ProcessId, ProcessScope, RetryPolicy, ReviewId, SagaStatus, StepId, StepPlan, TenantId,
    apply_action_result, apply_manual_resolution, start,
};
use thiserror::Error;

#[derive(Debug, Error)]
enum ExampleError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("trade did not enter the expected manual-review state")]
    UnexpectedState,
    #[error("saga did not plan the expected action")]
    MissingAction,
}

fn identifier<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn action_id(decision: &penelope::SagaDecision) -> Result<ActionId, ExampleError> {
    decision
        .next_action
        .as_ref()
        .map(|action| action.action_id.clone())
        .ok_or(ExampleError::MissingAction)
}

fn main() -> Result<(), ExampleError> {
    let policy = RetryPolicy::no_retry();
    let definition = LinearSagaDefinition::new(
        identifier::<DefinitionId>("def_trade")?,
        identifier::<DefinitionVersion>("dfv_one")?,
        ContentDigest([99; 32]),
        vec![
            StepPlan::canonical_command(
                identifier::<StepId>("stp_lock_seller")?,
                ContentDigest([1; 32]),
                policy,
            )
            .with_compensation(CompensationPlan::canonical_command(
                ContentDigest([9; 32]),
                policy,
            )),
            StepPlan::canonical_command(
                identifier::<StepId>("stp_settle")?,
                ContentDigest([2; 32]),
                policy,
            ),
        ],
    );
    let tenant_id = identifier::<TenantId>("tnt_market")?;
    let process_id = identifier::<ProcessId>("prc_trade_review")?;
    let lock = start(
        &definition,
        tenant_id.clone(),
        process_id.clone(),
        identifier("act_lock_seller")?,
    )?;
    let settle = apply_action_result(
        &definition,
        &lock.projection,
        tenant_id.clone(),
        process_id.clone(),
        &ActionResultObservation::succeeded(action_id(&lock)?),
        Some(identifier("act_settle")?),
    )?;
    let escalated = apply_action_result(
        &definition,
        &settle.projection,
        tenant_id.clone(),
        process_id.clone(),
        &ActionResultObservation::unknown(action_id(&settle)?),
        None,
    )?;
    if escalated.projection.status != SagaStatus::Escalated {
        return Err(ExampleError::UnexpectedState);
    }
    let scope = ProcessScope::new(
        tenant_id.clone(),
        process_id.clone(),
        definition.definition_id.clone(),
        definition.definition_version.clone(),
        definition.definition_digest,
    );
    // In a durable composition root this is written atomically with the
    // `ReviewOpened` outcome, before the review queue is notified.
    let review = escalated.manual_review_request(
        &scope,
        identifier::<ReviewId>("rev_trade_settlement_unknown")?,
        9,
        Some(penelope::LogicalTime(10_000)),
        ContentDigest([77; 32]),
    )?;
    review.validate()?;

    // The review port has verified the authority/evidence for this decision.
    let compensation = apply_manual_resolution(
        &definition,
        &escalated.projection,
        tenant_id.clone(),
        process_id.clone(),
        ManualReviewResolution::Compensate,
        Some(identifier("act_unlock_seller")?),
    )?;
    let completed = apply_action_result(
        &definition,
        &compensation.projection,
        tenant_id,
        process_id,
        &ActionResultObservation::succeeded(action_id(&compensation)?),
        None,
    )?;
    if completed.projection.status == SagaStatus::Compensated {
        Ok(())
    } else {
        Err(ExampleError::UnexpectedState)
    }
}
