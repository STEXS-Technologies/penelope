//! A pure `trade.v1` known-failure and compensation simulation.
//!
//! A real adapter must durably append every decision and obtain verified
//! canonical evidence before passing an observation to the pure engine.

use penelope::saga::{
    ActionResultObservation, CompensationPlan, LinearSagaDefinition, LinearSagaInput, RetryPolicy,
    SagaStatus, StepPlan, prepare_linear,
};
use penelope::{
    ActionId, ContentDigest, DefinitionId, DefinitionVersion, DomainError, ProcessId, StepId,
    TenantId,
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

    let (_, lock) = prepare_linear(
        &definition,
        None,
        &tenant_id,
        &process_id,
        &LinearSagaInput::Start {
            input_id: identifier("inp_lock")?,
            action_id: identifier("act_lock_seller")?,
        },
    )?
    .into_parts();
    let (_, settle) = prepare_linear(
        &definition,
        Some(&lock.projection),
        &tenant_id,
        &process_id,
        &LinearSagaInput::ActionResult {
            input_id: identifier("inp_lock_result")?,
            observation: ActionResultObservation::succeeded(action_id(&lock)?),
            next_action_id: Some(identifier("act_settle")?),
        },
    )?
    .into_parts();
    let (_, unlock) = prepare_linear(
        &definition,
        Some(&settle.projection),
        &tenant_id,
        &process_id,
        &LinearSagaInput::ActionResult {
            input_id: identifier("inp_settle_result")?,
            observation: ActionResultObservation::terminal_failure(action_id(&settle)?),
            next_action_id: Some(identifier("act_unlock_seller")?),
        },
    )?
    .into_parts();
    let (_, completed) = prepare_linear(
        &definition,
        Some(&unlock.projection),
        &tenant_id,
        &process_id,
        &LinearSagaInput::ActionResult {
            input_id: identifier("inp_unlock_result")?,
            observation: ActionResultObservation::succeeded(action_id(&unlock)?),
            next_action_id: None,
        },
    )?
    .into_parts();

    if completed.projection.status == SagaStatus::Compensated {
        Ok(())
    } else {
        Err(ExampleError::NotCompensated)
    }
}
