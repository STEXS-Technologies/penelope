//! Deterministic execution for bounded process graphs.

use crate::engine::{
    ActionResultObservationV1, ActionResultV1, GraphTransitionOutcomeV1, ProcessGraphDefinitionV1,
    SagaStatusV1,
};
use penelope_domain::{
    ActionId, InputId, ProcessActionDtoV1, ProcessId, ProcessScopeV1, StepId, TenantId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Replayable projection for a graph process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSagaProjectionV1 {
    /// Pinned process scope.
    pub scope: ProcessScopeV1,
    /// Current step awaiting an observation.
    pub current_step_id: Option<StepId>,
    /// Number of step actions issued so far.
    pub step_visits: u32,
    /// Active action identity, if the process is running.
    pub active_action_id: Option<ActionId>,
    /// Every action identity issued by this process.
    pub issued_action_ids: Vec<ActionId>,
    /// Current lifecycle status.
    pub status: SagaStatusV1,
}

/// Typed graph input accepted by the deterministic executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphSagaInputV1 {
    /// Starts a graph at its declared entry step.
    Start {
        /// Immutable inbox identity.
        input_id: InputId,
        /// Fresh action identity for the entry step.
        action_id: ActionId,
    },
    /// Applies a result to the currently active graph action.
    ActionResult {
        /// Immutable inbox identity.
        input_id: InputId,
        /// Correlated result observation.
        observation: ActionResultObservationV1,
        /// Fresh action identity for a selected destination step.
        next_action_id: Option<ActionId>,
    },
}

/// Immutable graph event suitable for an append-only process log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphSagaEventV1 {
    /// Recorded process start.
    Started {
        /// Accepted input identity.
        input_id: InputId,
        /// Entry action identity.
        action_id: ActionId,
    },
    /// Recorded action result and selected next action identity.
    ActionResultObserved {
        /// Accepted input identity.
        input_id: InputId,
        /// Correlated result.
        observation: ActionResultObservationV1,
        /// Destination action identity, when another step is selected.
        next_action_id: Option<ActionId>,
    },
}

/// Graph execution failure.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum GraphEngineError {
    /// Graph definition failed validation.
    #[error("graph definition is invalid")]
    InvalidDefinition {
        /// Underlying graph-definition invariant failure.
        #[source]
        source: crate::engine::GraphDefinitionError,
    },
    /// A start was requested for an existing process.
    #[error("graph process is already started")]
    AlreadyStarted,
    /// A result was supplied without a running process.
    #[error("graph process is not running")]
    NotRunning,
    /// Result action does not match the active action.
    #[error("graph result action does not match the active action")]
    ActionMismatch,
    /// Input identity was already accepted by the process.
    #[error("graph input identity was already accepted")]
    DuplicateInput,
    /// A destination action identity was missing or unexpectedly present.
    #[error("graph transition action identity is inconsistent")]
    ActionIdentityMismatch,
    /// A fresh action identity was already issued by this process.
    #[error("graph action identity was already issued")]
    DuplicateActionId,
    /// The graph supplied no edge for the observed result.
    #[error("graph has no transition for the observed result")]
    MissingTransition,
    /// The graph's explicit visit bound was exceeded.
    #[error("graph step visit limit was exceeded")]
    VisitLimitExceeded,
    /// Event replay did not match the pinned process scope.
    #[error("graph replay process scope does not match")]
    ScopeMismatch,
}

/// Result of one pure graph decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSagaDecisionV1 {
    /// Projection after applying the decision.
    pub projection: GraphSagaProjectionV1,
    /// Action to dispatch, if the process remains runnable.
    pub next_action: Option<ProcessActionDtoV1>,
    /// Immutable event to append before dispatching the action.
    pub event: GraphSagaEventV1,
}

fn action_for(
    definition: &ProcessGraphDefinitionV1,
    scope: &ProcessScopeV1,
    step_id: &StepId,
    action_id: ActionId,
    visit: u32,
) -> Result<ProcessActionDtoV1, GraphEngineError> {
    let step = definition
        .steps
        .iter()
        .find(|step| &step.step_id == step_id)
        .ok_or(GraphEngineError::ActionIdentityMismatch)?;
    Ok(ProcessActionDtoV1::new(
        scope.clone(),
        action_id,
        step.step_id.clone(),
        visit.saturating_sub(1),
        step.action_kind,
        step.payload_digest,
    ))
}

const fn transition_outcome(result: ActionResultV1) -> GraphTransitionOutcomeV1 {
    match result {
        ActionResultV1::Succeeded => GraphTransitionOutcomeV1::Succeeded,
        ActionResultV1::RetryableFailure => GraphTransitionOutcomeV1::RetryableFailure,
        ActionResultV1::TerminalFailure | ActionResultV1::Unknown => {
            GraphTransitionOutcomeV1::TerminalFailure
        }
    }
}

fn ensure_new_action(
    projection: &GraphSagaProjectionV1,
    next_action_id: Option<&ActionId>,
) -> Result<ActionId, GraphEngineError> {
    let action_id = next_action_id.ok_or(GraphEngineError::ActionIdentityMismatch)?;
    if projection
        .issued_action_ids
        .iter()
        .any(|issued| issued == action_id)
    {
        return Err(GraphEngineError::DuplicateActionId);
    }
    Ok(action_id.clone())
}

/// Starts a graph process at its validated entry step.
///
/// # Errors
///
/// Returns a typed error when the graph or start input is invalid.
pub fn start_graph(
    definition: &ProcessGraphDefinitionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    input: GraphSagaInputV1,
) -> Result<GraphSagaDecisionV1, GraphEngineError> {
    definition
        .validate()
        .map_err(|source| GraphEngineError::InvalidDefinition { source })?;
    let GraphSagaInputV1::Start {
        input_id,
        action_id,
    } = input
    else {
        return Err(GraphEngineError::AlreadyStarted);
    };
    let scope = ProcessScopeV1::new(
        tenant_id,
        process_id,
        definition.definition_id.clone(),
        definition.definition_version.clone(),
        definition.definition_digest,
    );
    let action = action_for(
        definition,
        &scope,
        &definition.entry_step_id,
        action_id.clone(),
        1,
    )?;
    let projection = GraphSagaProjectionV1 {
        scope,
        current_step_id: Some(definition.entry_step_id.clone()),
        step_visits: 1,
        active_action_id: Some(action_id.clone()),
        issued_action_ids: vec![action_id.clone()],
        status: SagaStatusV1::Running,
    };
    Ok(GraphSagaDecisionV1 {
        projection,
        next_action: Some(action),
        event: GraphSagaEventV1::Started {
            input_id,
            action_id,
        },
    })
}

/// Applies one correlated graph action result.
///
/// # Errors
///
/// Returns a typed error when the definition, projection, result identity, or
/// selected transition violates a graph invariant.
pub fn apply_graph_result(
    definition: &ProcessGraphDefinitionV1,
    projection: &GraphSagaProjectionV1,
    input: GraphSagaInputV1,
) -> Result<GraphSagaDecisionV1, GraphEngineError> {
    definition
        .validate()
        .map_err(|source| GraphEngineError::InvalidDefinition { source })?;
    let GraphSagaInputV1::ActionResult {
        input_id,
        observation,
        next_action_id,
    } = input
    else {
        return Err(GraphEngineError::NotRunning);
    };
    if projection.status != SagaStatusV1::Running {
        return Err(GraphEngineError::NotRunning);
    }
    if projection
        .active_action_id
        .as_ref()
        .is_none_or(|active| active != &observation.action_id)
    {
        return Err(GraphEngineError::ActionMismatch);
    }
    let step_id = projection
        .current_step_id
        .as_ref()
        .ok_or(GraphEngineError::NotRunning)?;
    if observation.result == ActionResultV1::Unknown {
        if next_action_id.is_some() {
            return Err(GraphEngineError::ActionIdentityMismatch);
        }
        let mut escalated = projection.clone();
        escalated.current_step_id = None;
        escalated.active_action_id = None;
        escalated.status = SagaStatusV1::Escalated;
        return Ok(GraphSagaDecisionV1 {
            projection: escalated,
            next_action: None,
            event: GraphSagaEventV1::ActionResultObserved {
                input_id,
                observation,
                next_action_id,
            },
        });
    }
    let outcome = transition_outcome(observation.result);
    let transition = definition
        .transitions
        .iter()
        .find(|edge| &edge.from_step == step_id && edge.on == outcome)
        .ok_or(GraphEngineError::MissingTransition)?;
    let mut next_projection = projection.clone();
    next_projection.active_action_id = None;
    let next_action = if let Some(to_step) = &transition.to_step {
        let action_id = ensure_new_action(projection, next_action_id.as_ref())?;
        let next_visits = projection
            .step_visits
            .checked_add(1)
            .ok_or(GraphEngineError::VisitLimitExceeded)?;
        if next_visits > definition.max_step_visits.get() {
            return Err(GraphEngineError::VisitLimitExceeded);
        }
        let action = action_for(
            definition,
            &projection.scope,
            to_step,
            action_id.clone(),
            next_visits,
        )?;
        next_projection.current_step_id = Some(to_step.clone());
        next_projection.step_visits = next_visits;
        next_projection.active_action_id = Some(action_id.clone());
        next_projection.issued_action_ids.push(action_id);
        Some(action)
    } else {
        if next_action_id.is_some() {
            return Err(GraphEngineError::ActionIdentityMismatch);
        }
        next_projection.current_step_id = None;
        next_projection.status = if observation.result == ActionResultV1::Succeeded {
            SagaStatusV1::Completed
        } else {
            SagaStatusV1::Escalated
        };
        None
    };
    Ok(GraphSagaDecisionV1 {
        projection: next_projection,
        next_action,
        event: GraphSagaEventV1::ActionResultObserved {
            input_id,
            observation,
            next_action_id,
        },
    })
}

/// Replays graph events into a deterministic projection.
///
/// # Errors
///
/// Returns a typed error when event order, input identity, scope, or graph
/// transition invariants fail during replay.
pub fn replay_graph(
    definition: &ProcessGraphDefinitionV1,
    tenant_id: &TenantId,
    process_id: &ProcessId,
    events: &[GraphSagaEventV1],
) -> Result<GraphSagaProjectionV1, GraphEngineError> {
    let mut projection = None;
    let mut accepted_inputs = Vec::new();
    for event in events {
        let observed_input_id = match event {
            GraphSagaEventV1::Started { input_id, .. }
            | GraphSagaEventV1::ActionResultObserved { input_id, .. } => input_id,
        };
        if accepted_inputs
            .iter()
            .any(|accepted: &InputId| accepted == observed_input_id)
        {
            return Err(GraphEngineError::DuplicateInput);
        }
        accepted_inputs.push(observed_input_id.clone());
        projection = Some(match (projection, event) {
            (
                None,
                GraphSagaEventV1::Started {
                    input_id: event_input_id,
                    action_id,
                },
            ) => {
                start_graph(
                    definition,
                    tenant_id.clone(),
                    process_id.clone(),
                    GraphSagaInputV1::Start {
                        input_id: event_input_id.clone(),
                        action_id: action_id.clone(),
                    },
                )?
                .projection
            }
            (
                Some(current),
                GraphSagaEventV1::ActionResultObserved {
                    input_id: event_input_id,
                    observation,
                    next_action_id,
                },
            ) => {
                apply_graph_result(
                    definition,
                    &current,
                    GraphSagaInputV1::ActionResult {
                        input_id: event_input_id.clone(),
                        observation: observation.clone(),
                        next_action_id: next_action_id.clone(),
                    },
                )?
                .projection
            }
            _ => return Err(GraphEngineError::AlreadyStarted),
        });
    }
    projection.ok_or(GraphEngineError::NotRunning)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::engine::{GraphTransitionV1, RetryPolicyV1, StepPlanV1};
    use penelope_domain::{ContentDigest, DefinitionId, DefinitionVersion, ProcessId, TenantId};

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    fn definition() -> ProcessGraphDefinitionV1 {
        let policy = RetryPolicyV1::no_retry();
        ProcessGraphDefinitionV1 {
            definition_id: id::<DefinitionId>("def_graph_exec"),
            definition_version: id::<DefinitionVersion>("dfv_one"),
            definition_digest: ContentDigest([1; 32]),
            steps: vec![
                StepPlanV1::canonical_command(id("stp_first"), ContentDigest([2; 32]), policy),
                StepPlanV1::canonical_command(id("stp_second"), ContentDigest([3; 32]), policy),
            ],
            entry_step_id: id("stp_first"),
            transitions: vec![
                GraphTransitionV1 {
                    from_step: id("stp_first"),
                    on: GraphTransitionOutcomeV1::Succeeded,
                    to_step: Some(id("stp_second")),
                },
                GraphTransitionV1 {
                    from_step: id("stp_second"),
                    on: GraphTransitionOutcomeV1::Succeeded,
                    to_step: None,
                },
            ],
            max_step_visits: std::num::NonZeroU32::new(2).unwrap(),
        }
    }

    #[test]
    fn graph_start_result_and_replay_converge() {
        let definition = definition();
        let tenant_id = id::<TenantId>("tnt_graph_exec");
        let process_id = id::<ProcessId>("prc_graph_exec");
        let started = start_graph(
            &definition,
            tenant_id.clone(),
            process_id.clone(),
            GraphSagaInputV1::Start {
                input_id: id("inp_graph_start"),
                action_id: id("act_graph_first"),
            },
        )
        .unwrap();
        let finished = apply_graph_result(
            &definition,
            &started.projection,
            GraphSagaInputV1::ActionResult {
                input_id: id("inp_graph_first_result"),
                observation: ActionResultObservationV1::succeeded(id("act_graph_first")),
                next_action_id: Some(id("act_graph_second")),
            },
        )
        .unwrap();
        let completed = apply_graph_result(
            &definition,
            &finished.projection,
            GraphSagaInputV1::ActionResult {
                input_id: id("inp_graph_second_result"),
                observation: ActionResultObservationV1::succeeded(id("act_graph_second")),
                next_action_id: None,
            },
        )
        .unwrap();
        assert_eq!(completed.projection.status, SagaStatusV1::Completed);
        let replayed = replay_graph(
            &definition,
            &tenant_id,
            &process_id,
            &[started.event, finished.event, completed.event],
        )
        .unwrap();
        assert_eq!(replayed, completed.projection);
    }

    #[test]
    fn graph_rejects_result_for_wrong_action() {
        let definition = definition();
        let started = start_graph(
            &definition,
            id("tnt_graph_exec"),
            id("prc_graph_exec"),
            GraphSagaInputV1::Start {
                input_id: id("inp_graph_start"),
                action_id: id("act_graph_first"),
            },
        )
        .unwrap();
        let error = apply_graph_result(
            &definition,
            &started.projection,
            GraphSagaInputV1::ActionResult {
                input_id: id("inp_graph_wrong_result"),
                observation: ActionResultObservationV1::succeeded(id("act_other")),
                next_action_id: Some(id("act_graph_second")),
            },
        )
        .unwrap_err();
        assert_eq!(error, GraphEngineError::ActionMismatch);
    }

    #[test]
    fn graph_escalates_unknown_without_following_an_edge() {
        let definition = definition();
        let started = start_graph(
            &definition,
            id("tnt_graph_exec"),
            id("prc_graph_exec"),
            GraphSagaInputV1::Start {
                input_id: id("inp_graph_start"),
                action_id: id("act_graph_first"),
            },
        )
        .unwrap();
        let decision = apply_graph_result(
            &definition,
            &started.projection,
            GraphSagaInputV1::ActionResult {
                input_id: id("inp_graph_unknown"),
                observation: ActionResultObservationV1 {
                    action_id: id("act_graph_first"),
                    result: ActionResultV1::Unknown,
                },
                next_action_id: None,
            },
        )
        .unwrap();
        assert_eq!(decision.projection.status, SagaStatusV1::Escalated);
        assert!(decision.next_action.is_none());
    }
}
