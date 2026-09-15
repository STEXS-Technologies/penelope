//! Deterministic execution for bounded process graphs.

use crate::engine::{
    ActionResult, ActionResultObservation, GraphTransitionOutcome, ProcessGraphDefinition,
    SagaStatus,
};
use penelope_domain::{
    ActionId, InputId, ProcessAction, ProcessId, ProcessScope, StepId, TenantId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// Maximum number of graph events accepted by one replay operation.
pub const MAX_GRAPH_REPLAY_EVENTS: usize = 4_096;

/// Replayable projection for a graph process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSagaProjection {
    /// Pinned process scope.
    pub scope: ProcessScope,
    /// Current step awaiting an observation.
    pub current_step_id: Option<StepId>,
    /// Number of step actions issued so far.
    pub step_visits: u32,
    /// Active action identity, if the process is running.
    pub active_action_id: Option<ActionId>,
    /// Every action identity issued by this process.
    pub issued_action_ids: Vec<ActionId>,
    /// Current lifecycle status.
    pub status: SagaStatus,
}

/// Typed graph input accepted by the deterministic executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphSagaInput {
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
        observation: ActionResultObservation,
        /// Fresh action identity for a selected destination step.
        next_action_id: Option<ActionId>,
    },
}

/// Immutable graph event suitable for an append-only process log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphSagaEvent {
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
        observation: ActionResultObservation,
        /// Destination action identity, when another step is selected.
        next_action_id: Option<ActionId>,
    },
}

/// Ordered envelope for graph events in a durable process log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSagaEventEnvelope {
    /// Zero-based contiguous position in the process log.
    pub sequence: u64,
    /// Immutable graph event at this position.
    pub event: GraphSagaEvent,
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
    /// Event sequence was not zero-based and contiguous.
    #[error("graph replay event sequence is not contiguous")]
    SequenceMismatch,
    /// Replay input exceeded the bounded process-log limit.
    #[error("graph replay event limit was exceeded")]
    ReplayLimitExceeded,
}

/// Result of one pure graph decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSagaDecision {
    /// Projection after applying the decision.
    pub projection: GraphSagaProjection,
    /// Action to dispatch, if the process remains runnable.
    pub next_action: Option<ProcessAction>,
    /// Immutable event to append before dispatching the action.
    pub event: GraphSagaEvent,
}

fn action_for(
    definition: &ProcessGraphDefinition,
    scope: &ProcessScope,
    step_id: &StepId,
    action_id: ActionId,
    visit: u32,
) -> Result<ProcessAction, GraphEngineError> {
    let step = definition
        .steps
        .iter()
        .find(|step| &step.step_id == step_id)
        .ok_or(GraphEngineError::ActionIdentityMismatch)?;
    Ok(ProcessAction::new(
        scope.clone(),
        action_id,
        step.step_id.clone(),
        visit.saturating_sub(1),
        step.action_kind,
        step.payload_digest,
    ))
}

const fn transition_outcome(result: ActionResult) -> GraphTransitionOutcome {
    match result {
        ActionResult::Succeeded => GraphTransitionOutcome::Succeeded,
        ActionResult::RetryableFailure => GraphTransitionOutcome::RetryableFailure,
        ActionResult::TerminalFailure | ActionResult::Unknown => {
            GraphTransitionOutcome::TerminalFailure
        }
    }
}

fn ensure_new_action(
    projection: &GraphSagaProjection,
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
    definition: &ProcessGraphDefinition,
    tenant_id: TenantId,
    process_id: ProcessId,
    input: GraphSagaInput,
) -> Result<GraphSagaDecision, GraphEngineError> {
    definition
        .validate()
        .map_err(|source| GraphEngineError::InvalidDefinition { source })?;
    let GraphSagaInput::Start {
        input_id,
        action_id,
    } = input
    else {
        return Err(GraphEngineError::AlreadyStarted);
    };
    let scope = ProcessScope::new(
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
    let projection = GraphSagaProjection {
        scope,
        current_step_id: Some(definition.entry_step_id.clone()),
        step_visits: 1,
        active_action_id: Some(action_id.clone()),
        issued_action_ids: vec![action_id.clone()],
        status: SagaStatus::Running,
    };
    Ok(GraphSagaDecision {
        projection,
        next_action: Some(action),
        event: GraphSagaEvent::Started {
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
    definition: &ProcessGraphDefinition,
    projection: &GraphSagaProjection,
    input: GraphSagaInput,
) -> Result<GraphSagaDecision, GraphEngineError> {
    definition
        .validate()
        .map_err(|source| GraphEngineError::InvalidDefinition { source })?;
    let GraphSagaInput::ActionResult {
        input_id,
        observation,
        next_action_id,
    } = input
    else {
        return Err(GraphEngineError::NotRunning);
    };
    if projection.status != SagaStatus::Running {
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
    if observation.result == ActionResult::Unknown {
        if next_action_id.is_some() {
            return Err(GraphEngineError::ActionIdentityMismatch);
        }
        let mut escalated = projection.clone();
        escalated.current_step_id = None;
        escalated.active_action_id = None;
        escalated.status = SagaStatus::Escalated;
        return Ok(GraphSagaDecision {
            projection: escalated,
            next_action: None,
            event: GraphSagaEvent::ActionResultObserved {
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
        next_projection.status = if observation.result == ActionResult::Succeeded {
            SagaStatus::Completed
        } else {
            SagaStatus::Escalated
        };
        None
    };
    Ok(GraphSagaDecision {
        projection: next_projection,
        next_action,
        event: GraphSagaEvent::ActionResultObserved {
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
    definition: &ProcessGraphDefinition,
    tenant_id: &TenantId,
    process_id: &ProcessId,
    events: &[GraphSagaEvent],
) -> Result<GraphSagaProjection, GraphEngineError> {
    if events.len() > MAX_GRAPH_REPLAY_EVENTS {
        return Err(GraphEngineError::ReplayLimitExceeded);
    }
    let mut projection = None;
    let mut accepted_inputs = HashSet::new();
    for event in events {
        let observed_input_id = match event {
            GraphSagaEvent::Started { input_id, .. }
            | GraphSagaEvent::ActionResultObserved { input_id, .. } => input_id,
        };
        if !accepted_inputs.insert(observed_input_id.clone()) {
            return Err(GraphEngineError::DuplicateInput);
        }
        projection = Some(match (projection, event) {
            (
                None,
                GraphSagaEvent::Started {
                    input_id: event_input_id,
                    action_id,
                },
            ) => {
                start_graph(
                    definition,
                    tenant_id.clone(),
                    process_id.clone(),
                    GraphSagaInput::Start {
                        input_id: event_input_id.clone(),
                        action_id: action_id.clone(),
                    },
                )?
                .projection
            }
            (
                Some(current),
                GraphSagaEvent::ActionResultObserved {
                    input_id: event_input_id,
                    observation,
                    next_action_id,
                },
            ) => {
                apply_graph_result(
                    definition,
                    &current,
                    GraphSagaInput::ActionResult {
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

/// Replays graph events after validating their durable sequence envelope.
///
/// # Errors
///
/// Returns [`GraphEngineError::SequenceMismatch`] for gaps, duplicates, or
/// reordered envelopes; otherwise returns the same errors as [`replay_graph`].
pub fn replay_graph_ordered(
    definition: &ProcessGraphDefinition,
    tenant_id: &TenantId,
    process_id: &ProcessId,
    events: &[GraphSagaEventEnvelope],
) -> Result<GraphSagaProjection, GraphEngineError> {
    if events.len() > MAX_GRAPH_REPLAY_EVENTS {
        return Err(GraphEngineError::ReplayLimitExceeded);
    }
    for (expected, envelope) in events.iter().enumerate() {
        if envelope.sequence != expected as u64 {
            return Err(GraphEngineError::SequenceMismatch);
        }
    }
    let unwrapped: Vec<_> = events
        .iter()
        .map(|envelope| envelope.event.clone())
        .collect();
    replay_graph(definition, tenant_id, process_id, &unwrapped)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::engine::{GraphTransition, RetryPolicy, StepPlan};
    use penelope_domain::{ContentDigest, DefinitionId, DefinitionVersion, ProcessId, TenantId};

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    fn definition() -> ProcessGraphDefinition {
        let policy = RetryPolicy::no_retry();
        ProcessGraphDefinition {
            definition_id: id::<DefinitionId>("def_graph_exec"),
            definition_version: id::<DefinitionVersion>("dfv_one"),
            definition_digest: ContentDigest([1; 32]),
            steps: vec![
                StepPlan::canonical_command(id("stp_first"), ContentDigest([2; 32]), policy),
                StepPlan::canonical_command(id("stp_second"), ContentDigest([3; 32]), policy),
            ],
            entry_step_id: id("stp_first"),
            transitions: vec![
                GraphTransition {
                    from_step: id("stp_first"),
                    on: GraphTransitionOutcome::Succeeded,
                    to_step: Some(id("stp_second")),
                },
                GraphTransition {
                    from_step: id("stp_second"),
                    on: GraphTransitionOutcome::Succeeded,
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
            GraphSagaInput::Start {
                input_id: id("inp_graph_start"),
                action_id: id("act_graph_first"),
            },
        )
        .unwrap();
        let finished = apply_graph_result(
            &definition,
            &started.projection,
            GraphSagaInput::ActionResult {
                input_id: id("inp_graph_first_result"),
                observation: ActionResultObservation::succeeded(id("act_graph_first")),
                next_action_id: Some(id("act_graph_second")),
            },
        )
        .unwrap();
        let completed = apply_graph_result(
            &definition,
            &finished.projection,
            GraphSagaInput::ActionResult {
                input_id: id("inp_graph_second_result"),
                observation: ActionResultObservation::succeeded(id("act_graph_second")),
                next_action_id: None,
            },
        )
        .unwrap();
        assert_eq!(completed.projection.status, SagaStatus::Completed);
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
    fn ordered_graph_replay_rejects_sequence_gaps() {
        let definition = definition();
        let started = start_graph(
            &definition,
            id("tnt_graph"),
            id("prc_graph"),
            GraphSagaInput::Start {
                input_id: id("inp_start"),
                action_id: id("act_first"),
            },
        )
        .unwrap();
        let events = [GraphSagaEventEnvelope {
            sequence: 1,
            event: started.event,
        }];
        assert_eq!(
            replay_graph_ordered(&definition, &id("tnt_graph"), &id("prc_graph"), &events),
            Err(GraphEngineError::SequenceMismatch)
        );
    }

    #[test]
    fn graph_replay_rejects_oversized_logs_before_allocating_tracking_state() {
        let definition = definition();
        let started = start_graph(
            &definition,
            id("tnt_graph"),
            id("prc_graph"),
            GraphSagaInput::Start {
                input_id: id("inp_start"),
                action_id: id("act_first"),
            },
        )
        .unwrap();
        let events = vec![started.event; MAX_GRAPH_REPLAY_EVENTS + 1];
        assert_eq!(
            replay_graph(&definition, &id("tnt_graph"), &id("prc_graph"), &events),
            Err(GraphEngineError::ReplayLimitExceeded)
        );
    }

    #[test]
    fn graph_rejects_result_for_wrong_action() {
        let definition = definition();
        let started = start_graph(
            &definition,
            id("tnt_graph_exec"),
            id("prc_graph_exec"),
            GraphSagaInput::Start {
                input_id: id("inp_graph_start"),
                action_id: id("act_graph_first"),
            },
        )
        .unwrap();
        let error = apply_graph_result(
            &definition,
            &started.projection,
            GraphSagaInput::ActionResult {
                input_id: id("inp_graph_wrong_result"),
                observation: ActionResultObservation::succeeded(id("act_other")),
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
            GraphSagaInput::Start {
                input_id: id("inp_graph_start"),
                action_id: id("act_graph_first"),
            },
        )
        .unwrap();
        let decision = apply_graph_result(
            &definition,
            &started.projection,
            GraphSagaInput::ActionResult {
                input_id: id("inp_graph_unknown"),
                observation: ActionResultObservation {
                    action_id: id("act_graph_first"),
                    result: ActionResult::Unknown,
                },
                next_action_id: None,
            },
        )
        .unwrap();
        assert_eq!(decision.projection.status, SagaStatus::Escalated);
        assert!(decision.next_action.is_none());
    }
}
