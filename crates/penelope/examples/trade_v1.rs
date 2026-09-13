//! A pure `trade.v1` happy-path simulation.
//!
//! The integration layer must first durably record each decision, then submit
//! the returned action with its stable action ID. It may advance only after a
//! verified, correlated result has been appended to the outcome log.

use penelope::{
    ActionId, ActionResultObservationV1, ContentDigest, DomainError, LinearSagaDefinitionV1,
    ProcessActionKindV1, ProcessId, SagaStatusV1, StepId, StepPlanV1, TenantId,
    apply_action_result, start,
};

fn identifier<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let definition = LinearSagaDefinitionV1 {
        steps: vec![
            StepPlanV1 {
                step_id: identifier::<StepId>("stp_lock_seller")?,
                action_kind: ProcessActionKindV1::CanonicalCommand,
                payload_digest: ContentDigest([1; 32]),
            },
            StepPlanV1 {
                step_id: identifier::<StepId>("stp_lock_buyer")?,
                action_kind: ProcessActionKindV1::CanonicalCommand,
                payload_digest: ContentDigest([2; 32]),
            },
            StepPlanV1 {
                step_id: identifier::<StepId>("stp_settle")?,
                action_kind: ProcessActionKindV1::CanonicalCommand,
                payload_digest: ContentDigest([3; 32]),
            },
        ],
    };
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
        Err(Box::new(penelope::EngineError::TerminalProjection))
    }
}
