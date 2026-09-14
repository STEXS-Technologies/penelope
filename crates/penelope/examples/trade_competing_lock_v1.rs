//! A pure `trade.v1` competing-lock simulation.
//!
//! Two independent process instances request the same canonical asset lock.
//! StateChronicle is the authority that decides the competition. This example
//! receives only its two verified outcomes: one committed lock succeeds and
//! can progress toward settlement; the rejected lock becomes an escalation and
//! cannot issue a settlement action. No in-memory map here pretends to be a
//! canonical inventory implementation.

use penelope::{
    ActionId, ActionResultObservationV1, CanonicalCommandExpectationV1, CanonicalCommitId,
    CanonicalEventDtoV1, CanonicalEventId, ContentDigest, CorrelationError, DefinitionId,
    DefinitionVersion, DomainError, LinearSagaDefinitionV1, OperationId, ProcessId, ProcessScopeV1,
    ResourceId, RetryPolicyV1, SagaDecisionV1, SagaStatusV1, StepId, StepPlanV1, TenantId,
    apply_action_result, start, verify_committed_event,
};
use thiserror::Error;

#[derive(Debug, Error)]
enum ExampleError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Engine(#[from] penelope::EngineError),
    #[error(transparent)]
    Correlation(#[from] CorrelationError),
    #[error("trade winner did not advance to settlement")]
    WinnerDidNotAdvance,
    #[error("competing trade did not escalate after its rejected lock")]
    LoserDidNotEscalate,
    #[error("saga did not plan an action")]
    MissingAction,
}

fn identifier<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn active_action_id(decision: &SagaDecisionV1) -> Result<ActionId, ExampleError> {
    decision
        .next_action
        .as_ref()
        .map(|action| action.action_id.clone())
        .ok_or(ExampleError::MissingAction)
}

fn definition() -> Result<LinearSagaDefinitionV1, ExampleError> {
    Ok(LinearSagaDefinitionV1::new(
        identifier::<DefinitionId>("def_trade")?,
        identifier::<DefinitionVersion>("dfv_one")?,
        ContentDigest([99; 32]),
        vec![
            StepPlanV1::canonical_command(
                identifier::<StepId>("stp_lock_shared_asset")?,
                ContentDigest([1; 32]),
                RetryPolicyV1::no_retry(),
            ),
            StepPlanV1::canonical_command(
                identifier::<StepId>("stp_settle")?,
                ContentDigest([2; 32]),
                RetryPolicyV1::no_retry(),
            ),
        ],
    ))
}

fn main() -> Result<(), ExampleError> {
    let definition = definition()?;
    let tenant_id = identifier::<TenantId>("tnt_market")?;
    let winning_process_id = identifier::<ProcessId>("prc_trade_winner")?;
    let losing_process_id = identifier::<ProcessId>("prc_trade_loser")?;

    let winning_lock = start(
        &definition,
        tenant_id.clone(),
        winning_process_id.clone(),
        identifier("act_lock_winner")?,
    )?;
    let losing_lock = start(
        &definition,
        tenant_id.clone(),
        losing_process_id.clone(),
        identifier("act_lock_loser")?,
    )?;

    let winner_lock_action_id = active_action_id(&winning_lock)?;
    let operation = identifier::<OperationId>("op_lock_asset")?;
    let resource_id = identifier::<ResourceId>("res_shared_asset")?;
    let winner_scope = ProcessScopeV1::new(
        tenant_id.clone(),
        winning_process_id.clone(),
        definition.definition_id.clone(),
        definition.definition_version.clone(),
        definition.definition_digest,
    );
    let expectation = CanonicalCommandExpectationV1 {
        scope: winner_scope,
        action_id: winner_lock_action_id.clone(),
        operation: operation.clone(),
        resource_ids: vec![resource_id.clone()],
        expected_event_payload_digest: ContentDigest([3; 32]),
    };
    let committed_event = CanonicalEventDtoV1::new(
        tenant_id.clone(),
        identifier::<CanonicalEventId>("cev_lock_winner")?,
        winner_lock_action_id,
        identifier::<CanonicalCommitId>("cmt_lock_winner")?,
        1,
        operation,
        vec![resource_id],
        ContentDigest([3; 32]),
    )?;
    // The outer adapter passes this only after its trusted StateChronicle
    // committed-event stream yields the event. The verifier pins every
    // correlation coordinate before the saga may advance.
    let verified_event = verify_committed_event(&expectation, committed_event)?;
    let winner = apply_action_result(
        &definition,
        &winning_lock.projection,
        tenant_id.clone(),
        winning_process_id,
        &ActionResultObservationV1::succeeded(verified_event.event.action_id),
        Some(identifier("act_settle_winner")?),
    )?;
    let winner_settlement = winner
        .next_action
        .as_ref()
        .ok_or(ExampleError::MissingAction)?;
    if winner_settlement.step_id != identifier::<StepId>("stp_settle")? {
        return Err(ExampleError::WinnerDidNotAdvance);
    }

    // The same canonical authority rejects the losing command because the
    // resource is already locked. A terminal failure has no retry permission.
    let loser = apply_action_result(
        &definition,
        &losing_lock.projection,
        tenant_id,
        losing_process_id,
        &ActionResultObservationV1::terminal_failure(active_action_id(&losing_lock)?),
        None,
    )?;
    if loser.projection.status != SagaStatusV1::Escalated || loser.next_action.is_some() {
        return Err(ExampleError::LoserDidNotEscalate);
    }
    Ok(())
}
