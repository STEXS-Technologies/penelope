//! A pure `trade.v1` settlement-unknown and authorized-resolution simulation.
//!
//! The outer application must verify and durably record the manual-review
//! decision before giving this typed resolution to the pure engine.

use penelope::{
    ActionId, ActionResultObservationV1, CompensationPlanV1, ContentDigest, DefinitionId,
    DefinitionVersion, DomainError, EngineError, LinearSagaDefinitionV1, ManualReviewResolutionV1,
    ProcessId, ProcessScopeV1, RetryPolicyV1, ReviewId, SagaStatusV1, StepId, StepPlanV1, TenantId,
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

fn action_id(decision: &penelope::SagaDecisionV1) -> Result<ActionId, ExampleError> {
    decision
        .next_action
        .as_ref()
        .map(|action| action.action_id.clone())
        .ok_or(ExampleError::MissingAction)
}

fn main() -> Result<(), ExampleError> {
    let policy = RetryPolicyV1::no_retry();
    let definition = LinearSagaDefinitionV1::new(
        identifier::<DefinitionId>("def_trade")?,
        identifier::<DefinitionVersion>("dfv_one")?,
        ContentDigest([99; 32]),
        vec![
            StepPlanV1::canonical_command(
                identifier::<StepId>("stp_lock_seller")?,
                ContentDigest([1; 32]),
                policy,
            )
            .with_compensation(CompensationPlanV1::canonical_command(
                ContentDigest([9; 32]),
                policy,
            )),
            StepPlanV1::canonical_command(
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
        &ActionResultObservationV1::succeeded(action_id(&lock)?),
        Some(identifier("act_settle")?),
    )?;
    let escalated = apply_action_result(
        &definition,
        &settle.projection,
        tenant_id.clone(),
        process_id.clone(),
        &ActionResultObservationV1::unknown(action_id(&settle)?),
        None,
    )?;
    if escalated.projection.status != SagaStatusV1::Escalated {
        return Err(ExampleError::UnexpectedState);
    }
    let scope = ProcessScopeV1::new(
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
        Some(penelope::LogicalTimeV1(10_000)),
        ContentDigest([77; 32]),
    )?;
    review.validate()?;

    // The review port has verified the authority/evidence for this decision.
    let compensation = apply_manual_resolution(
        &definition,
        &escalated.projection,
        tenant_id.clone(),
        process_id.clone(),
        ManualReviewResolutionV1::Compensate,
        Some(identifier("act_unlock_seller")?),
    )?;
    let completed = apply_action_result(
        &definition,
        &compensation.projection,
        tenant_id,
        process_id,
        &ActionResultObservationV1::succeeded(action_id(&compensation)?),
        None,
    )?;
    if completed.projection.status == SagaStatusV1::Compensated {
        Ok(())
    } else {
        Err(ExampleError::UnexpectedState)
    }
}
