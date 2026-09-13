//! A pure `trade.v1` retry-timer simulation.
//!
//! The application must persist the resulting timer decision with its outcome
//! and action before calling a timer adapter. A timer delivery is an explicit
//! typed input; it cannot be substituted with a regular action result.

use std::num::{NonZeroU32, NonZeroU64};

use penelope::{
    ActionId, ActionResultObservationV1, ContentDigest, DefinitionId, DefinitionVersion,
    DomainError, EngineError, LinearSagaDefinitionV1, LogicalTimeV1, ProcessActionKindV1,
    ProcessId, RetryBackoffV1, RetryPolicyV1, SagaStatusV1, StepId, StepPlanV1, TenantId,
    apply_action_result, fire_retry_timer, schedule_retry_timer, start,
};
use thiserror::Error;

#[derive(Debug, Error)]
enum ExampleError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("retry timer was not planned")]
    MissingTimer,
    #[error("retry simulation did not complete")]
    NotCompleted,
}

fn identifier<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn main() -> Result<(), ExampleError> {
    let backoff = RetryBackoffV1::new(
        NonZeroU64::new(100).ok_or(ExampleError::MissingTimer)?,
        NonZeroU64::new(1_000).ok_or(ExampleError::MissingTimer)?,
    )?;
    let definition = LinearSagaDefinitionV1::new(
        identifier::<DefinitionId>("def_trade")?,
        identifier::<DefinitionVersion>("dfv_one")?,
        ContentDigest([99; 32]),
        vec![StepPlanV1::canonical_command(
            identifier::<StepId>("stp_settle")?,
            ContentDigest([1; 32]),
            RetryPolicyV1::new(NonZeroU32::new(2).ok_or(ExampleError::MissingTimer)?)
                .with_backoff(backoff),
        )],
    );
    let tenant_id = identifier::<TenantId>("tnt_market")?;
    let process_id = identifier::<ProcessId>("prc_trade_retry")?;
    let first = start(
        &definition,
        tenant_id.clone(),
        process_id.clone(),
        identifier::<ActionId>("act_settle_first")?,
    )?;
    let first_action = first
        .next_action
        .as_ref()
        .ok_or(ExampleError::MissingTimer)?;

    let waiting = schedule_retry_timer(
        &definition,
        &first.projection,
        tenant_id.clone(),
        process_id.clone(),
        &ActionResultObservationV1::retryable_failure(first_action.action_id.clone()),
        Some(identifier("act_settle_timer")?),
        LogicalTimeV1(10_000),
    )?;
    let timer = waiting
        .retry_timer_schedule()
        .ok_or(ExampleError::MissingTimer)?;
    if timer.action.kind != ProcessActionKindV1::Timer || timer.due_at != LogicalTimeV1(10_100) {
        return Err(ExampleError::MissingTimer);
    }

    // A real outer layer atomically persisted `waiting` and `timer` before
    // calling its timer adapter. The adapter now returns this typed firing.
    let retry = fire_retry_timer(
        &definition,
        &waiting.projection,
        tenant_id.clone(),
        process_id.clone(),
        &timer.action.action_id,
        timer.due_at,
        identifier("act_settle_retry")?,
    )?;
    let retry_action = retry
        .next_action
        .as_ref()
        .ok_or(ExampleError::MissingTimer)?;
    let complete = apply_action_result(
        &definition,
        &retry.projection,
        tenant_id,
        process_id,
        &ActionResultObservationV1::succeeded(retry_action.action_id.clone()),
        None,
    )?;
    if complete.projection.status == SagaStatusV1::Completed {
        Ok(())
    } else {
        Err(ExampleError::NotCompleted)
    }
}
