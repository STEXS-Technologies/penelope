//! A pure `trade.v1` happy-path simulation.
//!
//! The integration layer must first durably record each decision, then submit
//! the returned action with its stable action ID. It may advance only after a
//! verified, correlated result has been appended to the outcome log.

use penelope::{
    ActionId, ActionResultObservationV1, ContentDigest, DefinitionId, DefinitionVersion,
    DomainError, LinearSagaDefinitionV1, ProcessId, RetryPolicyV1, SagaStatusV1, StepId,
    StepPlanV1, TenantId, apply_action_result, start,
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
}

fn identifier<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
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
    let mut next_action_ids = [
        identifier::<ActionId>("act_lock_buyer")?,
        identifier::<ActionId>("act_settle")?,
    ]
    .into_iter();
    let mut decision = start(
        &definition,
        tenant_id.clone(),
        process_id.clone(),
        identifier("act_lock_seller")?,
    )?;

    while let Some(action) = decision.next_action.as_ref() {
        // A real adapter obtains this only after durable StateChronicle evidence.
        let observed = ActionResultObservationV1::succeeded(action.action_id.clone());
        decision = apply_action_result(
            &definition,
            &decision.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observed,
            next_action_ids.next(),
        )?;
    }

    if decision.projection.status == SagaStatusV1::Completed {
        Ok(())
    } else {
        Err(ExampleError::NotCompleted)
    }
}
