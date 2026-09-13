//! Deterministic linear saga planning with typed inputs and outputs.

use penelope_domain::{
    ActionId, ContentDigest, ProcessActionDtoV1, ProcessActionKindV1, ProcessId, StepId, TenantId,
};
use thiserror::Error;

/// A declared process step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepPlanV1 {
    /// Stable step identity from the pinned definition.
    pub step_id: StepId,
    /// The action category dispatched for this step.
    pub action_kind: ProcessActionKindV1,
    /// Digest of immutable action parameters.
    pub payload_digest: ContentDigest,
}

/// A deterministic, ordered saga definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearSagaDefinitionV1 {
    /// Steps execute in vector order.
    pub steps: Vec<StepPlanV1>,
}

/// A replayable process projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinearSagaProjectionV1 {
    /// Index of the current step.
    pub next_step_index: usize,
    /// Zero-based attempt for the current step.
    pub current_attempt: u32,
    /// Terminal/non-terminal process status.
    pub status: SagaStatusV1,
}

/// Process lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SagaStatusV1 {
    /// A step is pending or executing.
    Running,
    /// Every declared step succeeded.
    Completed,
    /// A terminal result requires human intervention.
    Escalated,
}

/// Observed terminal action result supplied as data to the pure engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionResultV1 {
    /// The current action completed successfully.
    Succeeded,
    /// The action failed and may be retried with a new idempotent attempt.
    RetryableFailure,
    /// The action failed permanently and requires escalation.
    TerminalFailure,
    /// External outcome is ambiguous and must be reconciled before retry.
    Unknown,
}

/// A deterministic decision from a transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SagaDecisionV1 {
    /// Resulting replayable projection.
    pub projection: LinearSagaProjectionV1,
    /// At most one next action for this linear reference engine.
    pub next_action: Option<ProcessActionDtoV1>,
}

/// Engine invariant failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// A definition has no executable steps.
    #[error("saga definition has no steps")]
    EmptyDefinition,
    /// A projection points outside the pinned definition.
    #[error("projection step index is outside the definition")]
    InvalidProjection,
    /// A transition was requested after a terminal state.
    #[error("saga is already terminal")]
    TerminalProjection,
    /// Retrying the action would overflow its bounded attempt number.
    #[error("action attempt number overflowed")]
    AttemptOverflow,
}

impl LinearSagaDefinitionV1 {
    /// Validates the definition's minimum executable invariant.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::EmptyDefinition`] when no step is declared.
    pub const fn validate(&self) -> Result<(), EngineError> {
        if self.steps.is_empty() {
            Err(EngineError::EmptyDefinition)
        } else {
            Ok(())
        }
    }
}

/// Plans the first durable action for a newly created process.
///
/// # Errors
///
/// Returns [`EngineError::EmptyDefinition`] when the definition has no steps.
pub fn start(
    definition: &LinearSagaDefinitionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    action_id: ActionId,
) -> Result<SagaDecisionV1, EngineError> {
    definition.validate()?;
    let projection = LinearSagaProjectionV1 {
        next_step_index: 0,
        current_attempt: 0,
        status: SagaStatusV1::Running,
    };
    Ok(SagaDecisionV1 {
        next_action: Some(action_for(
            definition,
            &projection,
            tenant_id,
            process_id,
            action_id,
        )?),
        projection,
    })
}

/// Applies one observed action result and plans the next action when safe.
///
/// An unknown result deliberately escalates: retries require reconciliation
/// evidence and cannot be inferred by this pure function.
///
/// # Errors
///
/// Returns an invariant error for invalid/terminal projections.
pub fn apply_result(
    definition: &LinearSagaDefinitionV1,
    projection: LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    next_action_id: Option<ActionId>,
    result: ActionResultV1,
) -> Result<SagaDecisionV1, EngineError> {
    definition.validate()?;
    if projection.status != SagaStatusV1::Running {
        return Err(EngineError::TerminalProjection);
    }
    if projection.next_step_index >= definition.steps.len() {
        return Err(EngineError::InvalidProjection);
    }
    match result {
        ActionResultV1::Succeeded => {
            let next_step_index = projection.next_step_index.saturating_add(1);
            if next_step_index == definition.steps.len() {
                Ok(SagaDecisionV1 {
                    projection: LinearSagaProjectionV1 {
                        next_step_index,
                        current_attempt: 0,
                        status: SagaStatusV1::Completed,
                    },
                    next_action: None,
                })
            } else {
                let next = LinearSagaProjectionV1 {
                    next_step_index,
                    current_attempt: 0,
                    status: SagaStatusV1::Running,
                };
                let action_id = next_action_id.ok_or(EngineError::InvalidProjection)?;
                Ok(SagaDecisionV1 {
                    next_action: Some(action_for(
                        definition, &next, tenant_id, process_id, action_id,
                    )?),
                    projection: next,
                })
            }
        }
        ActionResultV1::RetryableFailure => {
            let action_id = next_action_id.ok_or(EngineError::InvalidProjection)?;
            let current_attempt = projection
                .current_attempt
                .checked_add(1)
                .ok_or(EngineError::AttemptOverflow)?;
            let retry = LinearSagaProjectionV1 {
                next_step_index: projection.next_step_index,
                current_attempt,
                status: SagaStatusV1::Running,
            };
            Ok(SagaDecisionV1 {
                next_action: Some(action_for(
                    definition, &retry, tenant_id, process_id, action_id,
                )?),
                projection: retry,
            })
        }
        ActionResultV1::TerminalFailure | ActionResultV1::Unknown => Ok(SagaDecisionV1 {
            projection: LinearSagaProjectionV1 {
                next_step_index: projection.next_step_index,
                current_attempt: projection.current_attempt,
                status: SagaStatusV1::Escalated,
            },
            next_action: None,
        }),
    }
}

fn action_for(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    action_id: ActionId,
) -> Result<ProcessActionDtoV1, EngineError> {
    let step = definition
        .steps
        .get(projection.next_step_index)
        .ok_or(EngineError::InvalidProjection)?;
    Ok(ProcessActionDtoV1::new(
        tenant_id,
        process_id,
        action_id,
        step.step_id.clone(),
        projection.current_attempt,
        step.action_kind,
        step.payload_digest,
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    fn definition() -> LinearSagaDefinitionV1 {
        LinearSagaDefinitionV1 {
            steps: vec![
                StepPlanV1 {
                    step_id: id("stp_lock"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([1; 32]),
                },
                StepPlanV1 {
                    step_id: id("stp_settle"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([2; 32]),
                },
            ],
        }
    }

    #[test]
    fn success_plans_ordered_steps_then_completes() {
        let first = start(
            &definition(),
            id("tnt_game"),
            id("prc_trade"),
            id("act_lock"),
        )
        .unwrap();
        assert_eq!(first.next_action.as_ref().unwrap().step_id, id("stp_lock"));
        let second = apply_result(
            &definition(),
            first.projection,
            id("tnt_game"),
            id("prc_trade"),
            Some(id("act_settle")),
            ActionResultV1::Succeeded,
        )
        .unwrap();
        assert_eq!(
            second.next_action.as_ref().unwrap().step_id,
            id("stp_settle")
        );
        let complete = apply_result(
            &definition(),
            second.projection,
            id("tnt_game"),
            id("prc_trade"),
            None,
            ActionResultV1::Succeeded,
        )
        .unwrap();
        assert_eq!(complete.projection.status, SagaStatusV1::Completed);
    }

    #[test]
    fn unknown_result_escalates_instead_of_blind_retry() {
        let started = start(
            &definition(),
            id("tnt_game"),
            id("prc_trade"),
            id("act_lock"),
        )
        .unwrap();
        let decision = apply_result(
            &definition(),
            started.projection,
            id("tnt_game"),
            id("prc_trade"),
            None,
            ActionResultV1::Unknown,
        )
        .unwrap();
        assert_eq!(decision.projection.status, SagaStatusV1::Escalated);
        assert!(decision.next_action.is_none());
    }

    #[test]
    fn retry_creates_a_new_attempt_for_the_same_step() {
        let started = start(
            &definition(),
            id("tnt_game"),
            id("prc_trade"),
            id("act_lock"),
        )
        .unwrap();
        let retry = apply_result(
            &definition(),
            started.projection,
            id("tnt_game"),
            id("prc_trade"),
            Some(id("act_lock_retry")),
            ActionResultV1::RetryableFailure,
        )
        .unwrap();
        assert_eq!(retry.projection.current_attempt, 1);
        assert_eq!(retry.next_action.as_ref().unwrap().attempt, 1);
        assert_eq!(retry.next_action.as_ref().unwrap().step_id, id("stp_lock"));
    }

    proptest! {
        #[test]
        fn retry_attempt_is_monotonic_for_every_representable_attempt(
            current_attempt in 0_u32..u32::MAX,
        ) {
            let Some(expected_attempt) = current_attempt.checked_add(1) else {
                return Ok(());
            };
            let retry = apply_result(
                &definition(),
                LinearSagaProjectionV1 {
                    next_step_index: 0,
                    current_attempt,
                    status: SagaStatusV1::Running,
                },
                id("tnt_game"),
                id("prc_trade"),
                Some(id("act_lock_retry")),
                ActionResultV1::RetryableFailure,
            )
            .unwrap();
            prop_assert_eq!(retry.projection.next_step_index, 0);
            prop_assert_eq!(retry.projection.current_attempt, expected_attempt);
            prop_assert_eq!(retry.next_action.unwrap().attempt, expected_attempt);
        }
    }
}
