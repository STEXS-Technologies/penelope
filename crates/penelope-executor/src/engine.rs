//! Deterministic linear saga planning with typed inputs and outputs.

use penelope_domain::{
    ActionId, ContentDigest, ProcessActionDtoV1, ProcessActionKindV1, ProcessId, StepId, TenantId,
};
use std::num::NonZeroU32;
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
    /// Bounded retry behavior for this step.
    pub retry_policy: RetryPolicyV1,
}

/// Explicit retry bound for one action step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicyV1 {
    /// Total number of permitted attempts, including the first attempt.
    pub max_attempts: NonZeroU32,
}

impl RetryPolicyV1 {
    /// Creates a retry policy with the given total attempt bound.
    pub const fn new(max_attempts: NonZeroU32) -> Self {
        Self { max_attempts }
    }

    /// Creates a policy permitting exactly one attempt and no retry.
    pub const fn no_retry() -> Self {
        Self {
            max_attempts: NonZeroU32::MIN,
        }
    }
}

impl StepPlanV1 {
    /// Declares a canonical-command step with a typed retry bound.
    pub const fn canonical_command(
        step_id: StepId,
        payload_digest: ContentDigest,
        retry_policy: RetryPolicyV1,
    ) -> Self {
        Self {
            step_id,
            action_kind: ProcessActionKindV1::CanonicalCommand,
            payload_digest,
            retry_policy,
        }
    }
}

/// A deterministic, ordered saga definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearSagaDefinitionV1 {
    /// Steps execute in vector order.
    pub steps: Vec<StepPlanV1>,
}

/// A replayable process projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearSagaProjectionV1 {
    /// Index of the current step.
    pub next_step_index: usize,
    /// Zero-based attempt for the current step.
    pub current_attempt: u32,
    /// Stable identity of the action whose result may advance this projection.
    pub active_action_id: Option<ActionId>,
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

/// An action result correlated to the independently idempotent action that
/// produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResultObservationV1 {
    /// The action identity reported by the external effect boundary.
    pub action_id: ActionId,
    /// The classified external result.
    pub result: ActionResultV1,
}

/// One immutable event from the linear engine's ordered process log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinearSagaEventV1 {
    /// The durable process-start record and its first planned action identity.
    Started {
        /// Identity for the first independently idempotent action.
        action_id: ActionId,
    },
    /// A recorded, correlated action result and the identity already planned
    /// for the next action, if that transition needs one.
    ActionResultObserved {
        /// Result accepted from the effect/canonical evidence boundary.
        observation: ActionResultObservationV1,
        /// Persisted identity for the following action or retry.
        next_action_id: Option<ActionId>,
    },
}

impl ActionResultObservationV1 {
    /// Records a successful result for `action_id`.
    pub const fn succeeded(action_id: ActionId) -> Self {
        Self {
            action_id,
            result: ActionResultV1::Succeeded,
        }
    }

    /// Records a retryable failure for `action_id`.
    pub const fn retryable_failure(action_id: ActionId) -> Self {
        Self {
            action_id,
            result: ActionResultV1::RetryableFailure,
        }
    }

    /// Records an ambiguous result for `action_id`.
    pub const fn unknown(action_id: ActionId) -> Self {
        Self {
            action_id,
            result: ActionResultV1::Unknown,
        }
    }
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
    /// The observed result does not belong to the projection's active action.
    #[error("observed result does not match the active action")]
    UnexpectedAction,
    /// A non-terminal transition did not receive an ID for its next action.
    #[error("next action identity is required")]
    MissingNextAction,
    /// An ordered log had no durable start record.
    #[error("saga log has no start event")]
    MissingStart,
    /// An ordered log attempted to start an existing process again.
    #[error("saga log contains more than one start event")]
    DuplicateStart,
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
        active_action_id: Some(action_id),
        status: SagaStatusV1::Running,
    };
    Ok(SagaDecisionV1 {
        next_action: Some(action_for(definition, &projection, tenant_id, process_id)?),
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
pub fn apply_action_result(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    observation: &ActionResultObservationV1,
    next_action_id: Option<ActionId>,
) -> Result<SagaDecisionV1, EngineError> {
    definition.validate()?;
    if projection.status != SagaStatusV1::Running {
        return Err(EngineError::TerminalProjection);
    }
    if projection.next_step_index >= definition.steps.len() {
        return Err(EngineError::InvalidProjection);
    }
    if projection.active_action_id.as_ref() != Some(&observation.action_id) {
        return Err(EngineError::UnexpectedAction);
    }
    match observation.result {
        ActionResultV1::Succeeded => {
            let next_step_index = projection.next_step_index.saturating_add(1);
            if next_step_index == definition.steps.len() {
                Ok(SagaDecisionV1 {
                    projection: LinearSagaProjectionV1 {
                        next_step_index,
                        current_attempt: 0,
                        active_action_id: None,
                        status: SagaStatusV1::Completed,
                    },
                    next_action: None,
                })
            } else {
                let next = LinearSagaProjectionV1 {
                    next_step_index,
                    current_attempt: 0,
                    active_action_id: Some(next_action_id.ok_or(EngineError::MissingNextAction)?),
                    status: SagaStatusV1::Running,
                };
                Ok(SagaDecisionV1 {
                    next_action: Some(action_for(definition, &next, tenant_id, process_id)?),
                    projection: next,
                })
            }
        }
        ActionResultV1::RetryableFailure => {
            let step = definition
                .steps
                .get(projection.next_step_index)
                .ok_or(EngineError::InvalidProjection)?;
            let current_attempt = projection
                .current_attempt
                .checked_add(1)
                .ok_or(EngineError::AttemptOverflow)?;
            if current_attempt >= step.retry_policy.max_attempts.get() {
                return Ok(SagaDecisionV1 {
                    projection: LinearSagaProjectionV1 {
                        next_step_index: projection.next_step_index,
                        current_attempt: projection.current_attempt,
                        active_action_id: None,
                        status: SagaStatusV1::Escalated,
                    },
                    next_action: None,
                });
            }
            let retry = LinearSagaProjectionV1 {
                next_step_index: projection.next_step_index,
                current_attempt,
                active_action_id: Some(next_action_id.ok_or(EngineError::MissingNextAction)?),
                status: SagaStatusV1::Running,
            };
            Ok(SagaDecisionV1 {
                next_action: Some(action_for(definition, &retry, tenant_id, process_id)?),
                projection: retry,
            })
        }
        ActionResultV1::TerminalFailure | ActionResultV1::Unknown => Ok(SagaDecisionV1 {
            projection: LinearSagaProjectionV1 {
                next_step_index: projection.next_step_index,
                current_attempt: projection.current_attempt,
                active_action_id: None,
                status: SagaStatusV1::Escalated,
            },
            next_action: None,
        }),
    }
}

/// Rebuilds the current decision from an ordered immutable process log.
///
/// The returned action, if any, is only the final pending action. Calling this
/// function does not perform I/O or authorize a dispatcher to resend an
/// earlier effect; an outer durable worker must reconcile and dispatch it.
///
/// # Errors
///
/// Returns an invariant error if the log lacks a start event, has multiple
/// starts, or contains an invalid action transition.
pub fn replay(
    definition: &LinearSagaDefinitionV1,
    tenant_id: &TenantId,
    process_id: &ProcessId,
    events: &[LinearSagaEventV1],
) -> Result<SagaDecisionV1, EngineError> {
    let mut decision: Option<SagaDecisionV1> = None;
    for event in events {
        match event {
            LinearSagaEventV1::Started { action_id } => {
                if decision.is_some() {
                    return Err(EngineError::DuplicateStart);
                }
                decision = Some(start(
                    definition,
                    tenant_id.clone(),
                    process_id.clone(),
                    action_id.clone(),
                )?);
            }
            LinearSagaEventV1::ActionResultObserved {
                observation,
                next_action_id,
            } => {
                let current = decision.as_ref().ok_or(EngineError::MissingStart)?;
                decision = Some(apply_action_result(
                    definition,
                    &current.projection,
                    tenant_id.clone(),
                    process_id.clone(),
                    observation,
                    next_action_id.clone(),
                )?);
            }
        }
    }
    decision.ok_or(EngineError::MissingStart)
}

fn action_for(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
) -> Result<ProcessActionDtoV1, EngineError> {
    let step = definition
        .steps
        .get(projection.next_step_index)
        .ok_or(EngineError::InvalidProjection)?;
    Ok(ProcessActionDtoV1::new(
        tenant_id,
        process_id,
        projection
            .active_action_id
            .clone()
            .ok_or(EngineError::InvalidProjection)?,
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
        let retry_policy = RetryPolicyV1::new(NonZeroU32::new(2).unwrap());
        LinearSagaDefinitionV1 {
            steps: vec![
                StepPlanV1 {
                    step_id: id("stp_lock"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([1; 32]),
                    retry_policy,
                },
                StepPlanV1 {
                    step_id: id("stp_settle"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([2; 32]),
                    retry_policy,
                },
            ],
        }
    }

    fn observation(
        action: &ProcessActionDtoV1,
        result: ActionResultV1,
    ) -> ActionResultObservationV1 {
        ActionResultObservationV1 {
            action_id: action.action_id.clone(),
            result,
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
        let second = apply_action_result(
            &definition(),
            &first.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                first.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            Some(id("act_settle")),
        )
        .unwrap();
        assert_eq!(
            second.next_action.as_ref().unwrap().step_id,
            id("stp_settle")
        );
        let complete = apply_action_result(
            &definition(),
            &second.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                second.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            None,
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
        let decision = apply_action_result(
            &definition(),
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::Unknown,
            ),
            None,
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
        let retry = apply_action_result(
            &definition(),
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::RetryableFailure,
            ),
            Some(id("act_lock_retry")),
        )
        .unwrap();
        assert_eq!(retry.projection.current_attempt, 1);
        assert_eq!(retry.next_action.as_ref().unwrap().attempt, 1);
        assert_eq!(retry.next_action.as_ref().unwrap().step_id, id("stp_lock"));
    }

    #[test]
    fn exhausted_retry_policy_escalates_without_a_new_action() {
        let definition = LinearSagaDefinitionV1 {
            steps: vec![StepPlanV1::canonical_command(
                id("stp_lock"),
                ContentDigest([1; 32]),
                RetryPolicyV1::no_retry(),
            )],
        };
        let started = start(&definition, id("tnt_game"), id("prc_trade"), id("act_lock")).unwrap();
        let decision = apply_action_result(
            &definition,
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::RetryableFailure,
            ),
            Some(id("act_retry")),
        )
        .unwrap();
        assert_eq!(decision.projection.status, SagaStatusV1::Escalated);
        assert!(decision.next_action.is_none());
    }

    proptest! {
        #[test]
        fn retry_attempt_is_monotonic_for_every_representable_attempt(
            current_attempt in 0_u32..u32::MAX,
        ) {
            let Some(expected_attempt) = current_attempt.checked_add(1) else {
                return Ok(());
            };
            let mut definition = definition();
            let Some(step) = definition.steps.first_mut() else {
                return Ok(());
            };
            step.retry_policy = RetryPolicyV1::new(NonZeroU32::new(u32::MAX).unwrap());
            let retry = apply_action_result(
                &definition,
                &LinearSagaProjectionV1 {
                    next_step_index: 0,
                    current_attempt,
                    active_action_id: Some(id("act_lock")),
                    status: SagaStatusV1::Running,
                },
                id("tnt_game"),
                id("prc_trade"),
                &ActionResultObservationV1 {
                    action_id: id("act_lock"),
                    result: ActionResultV1::RetryableFailure,
                },
                Some(id("act_lock_retry")),
            )
            .unwrap();
            prop_assert_eq!(retry.projection.next_step_index, 0);
            prop_assert_eq!(retry.projection.current_attempt, expected_attempt);
            prop_assert_eq!(retry.next_action.unwrap().attempt, expected_attempt);
        }
    }

    #[test]
    fn result_for_a_different_action_cannot_advance_the_saga() {
        let started = start(
            &definition(),
            id("tnt_game"),
            id("prc_trade"),
            id("act_lock"),
        )
        .unwrap();
        let error = apply_action_result(
            &definition(),
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &ActionResultObservationV1 {
                action_id: id("act_other"),
                result: ActionResultV1::Succeeded,
            },
            Some(id("act_settle")),
        )
        .unwrap_err();
        assert_eq!(error, EngineError::UnexpectedAction);
    }

    #[test]
    fn replay_rebuilds_the_final_projection_from_immutable_events() {
        let replayed = replay(
            &definition(),
            &id("tnt_game"),
            &id("prc_trade"),
            &[
                LinearSagaEventV1::Started {
                    action_id: id("act_lock"),
                },
                LinearSagaEventV1::ActionResultObserved {
                    observation: ActionResultObservationV1::succeeded(id("act_lock")),
                    next_action_id: Some(id("act_settle")),
                },
                LinearSagaEventV1::ActionResultObserved {
                    observation: ActionResultObservationV1::succeeded(id("act_settle")),
                    next_action_id: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(replayed.projection.status, SagaStatusV1::Completed);
        assert!(replayed.next_action.is_none());
    }

    #[test]
    fn replay_rejects_a_second_start_record() {
        let error = replay(
            &definition(),
            &id("tnt_game"),
            &id("prc_trade"),
            &[
                LinearSagaEventV1::Started {
                    action_id: id("act_lock"),
                },
                LinearSagaEventV1::Started {
                    action_id: id("act_other"),
                },
            ],
        )
        .unwrap_err();
        assert_eq!(error, EngineError::DuplicateStart);
    }
}
