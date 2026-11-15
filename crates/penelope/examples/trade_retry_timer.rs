//! A pure `trade.v1` retry-timer simulation.
//!
//! The application must persist the resulting timer decision with its outcome
//! and action before calling a timer adapter. A timer delivery is an explicit
//! typed input; it cannot be substituted with a regular action result.

use std::num::{NonZeroU32, NonZeroU64};

use penelope::saga::{
    ActionResultObservation, EngineError, LinearSagaDefinition, LinearSagaInput, RetryBackoff,
    RetryJitterSeed, RetryPolicy, SagaStatus, StepPlan, prepare_linear,
};
use penelope::{
    ActionId, ContentDigest, DefinitionId, DefinitionVersion, DomainError, LogicalTime,
    ProcessActionKind, ProcessId, StepId, TenantId,
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
    let backoff = RetryBackoff::new(
        NonZeroU64::new(100).ok_or(ExampleError::MissingTimer)?,
        NonZeroU64::new(1_000).ok_or(ExampleError::MissingTimer)?,
    )?;
    let definition = LinearSagaDefinition::new(
        identifier::<DefinitionId>("def_trade")?,
        identifier::<DefinitionVersion>("dfv_one")?,
        ContentDigest([99; 32]),
        vec![StepPlan::canonical_command(
            identifier::<StepId>("stp_settle")?,
            ContentDigest([1; 32]),
            RetryPolicy::new(NonZeroU32::new(2).ok_or(ExampleError::MissingTimer)?)
                .with_backoff(backoff),
        )],
    );
    let tenant_id = identifier::<TenantId>("tnt_market")?;
    let process_id = identifier::<ProcessId>("prc_trade_retry")?;
    let (_, first) = prepare_linear(
        &definition,
        None,
        &tenant_id,
        &process_id,
        &LinearSagaInput::Start {
            input_id: identifier("inp_start")?,
            action_id: identifier::<ActionId>("act_settle_first")?,
        },
    )?
    .into_parts();
    let first_action = first
        .next_action
        .as_ref()
        .ok_or(ExampleError::MissingTimer)?;

    let (_, waiting) = prepare_linear(
        &definition,
        Some(&first.projection),
        &tenant_id,
        &process_id,
        &LinearSagaInput::RetryTimerScheduled {
            input_id: identifier("inp_retryable")?,
            observation: ActionResultObservation::retryable_failure(first_action.action_id.clone()),
            next_action_id: Some(identifier("act_settle_timer")?),
            observed_at: LogicalTime(10_000),
            jitter_seed: RetryJitterSeed::from_digest(ContentDigest([7; 32])),
            retry_deadline: Some(LogicalTime(10_100)),
        },
    )?
    .into_parts();
    let timer = waiting
        .retry_timer_schedule()
        .ok_or(ExampleError::MissingTimer)?;
    if timer.action.kind != ProcessActionKind::Timer || timer.due_at != LogicalTime(10_100) {
        return Err(ExampleError::MissingTimer);
    }

    // A real outer layer atomically persisted `waiting` and `timer` before
    // calling its timer adapter. The adapter now returns this typed firing.
    let (_, retry) = prepare_linear(
        &definition,
        Some(&waiting.projection),
        &tenant_id,
        &process_id,
        &LinearSagaInput::RetryTimerFired {
            input_id: identifier("inp_timer_fired")?,
            timer_action_id: timer.action.action_id.clone(),
            fired_at: timer.due_at,
            next_action_id: identifier("act_settle_retry")?,
        },
    )?
    .into_parts();
    let retry_action = retry
        .next_action
        .as_ref()
        .ok_or(ExampleError::MissingTimer)?;
    let (_, complete) = prepare_linear(
        &definition,
        Some(&retry.projection),
        &tenant_id,
        &process_id,
        &LinearSagaInput::ActionResult {
            input_id: identifier("inp_retry_result")?,
            observation: ActionResultObservation::succeeded(retry_action.action_id.clone()),
            next_action_id: None,
        },
    )?
    .into_parts();
    if complete.projection.status == SagaStatus::Completed {
        Ok(())
    } else {
        Err(ExampleError::NotCompleted)
    }
}
