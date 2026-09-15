//! A pure `trade.v1` known-failure and compensation simulation.
//!
//! A real adapter must durably append every decision and obtain verified
//! canonical evidence before passing an observation to the pure engine.

use penelope::{
    ActionId, ActionResultObservation, CompensationPlan, ContentDigest, DefinitionId,
    DefinitionVersion, DomainError, LinearSagaDefinition, ProcessId, RetryPolicy, SagaStatus,
    StepId, StepPlan, TenantId, apply_action_result, start,
};
use thiserror::Error;

#[derive(Debug, Error)]
enum ExampleError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Engine(#[from] penelope::EngineError),
    #[error("trade failure did not finish compensation")]
    NotCompensated,
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
    let process_id = identifier::<ProcessId>("prc_trade_failure")?;

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
    let unlock = apply_action_result(
        &definition,
        &settle.projection,
        tenant_id.clone(),
        process_id.clone(),
        &ActionResultObservation::terminal_failure(action_id(&settle)?),
        Some(identifier("act_unlock_seller")?),
    )?;
    let completed = apply_action_result(
        &definition,
        &unlock.projection,
        tenant_id,
        process_id,
        &ActionResultObservation::succeeded(action_id(&unlock)?),
        None,
    )?;

    if completed.projection.status == SagaStatus::Compensated {
        Ok(())
    } else {
        Err(ExampleError::NotCompensated)
    }
}
