//! Deterministic coordination of linear and graph sagas.
//!
//! Decisions plan actions; applications durably admit those plans before dispatch.
//! Replay never performs external effects. Unknown outcomes retain the existing
//! reconciliation and manual-review rules. These are the original saga types and
//! execution functions, with their existing protocol and replay bounds.

/// Deterministic linear-saga reference API.
pub use penelope_executor::engine::{
    ActionResult, ActionResultObservation, CompensationPlan, EngineError, GraphDefinitionError,
    GraphTransition, GraphTransitionOutcome, LinearSagaDefinition, LinearSagaEvent,
    LinearSagaEventEnvelope, LinearSagaInput, LinearSagaProjection, MAX_GRAPH_TRANSITIONS,
    ProcessGraphDefinition, RetryBackoff, RetryJitter, RetryJitterSeed, RetryPolicy,
    RetryTimerScheduleRequest, SagaDecision, SagaStatus, StepPlan, apply_action_result,
    apply_manual_resolution, decide, fire_retry_timer, replay, replay_ordered,
    schedule_retry_timer, start,
};
/// Deterministic bounded graph-process API.
pub use penelope_executor::graph::{
    GraphEngineError, GraphSagaDecision, GraphSagaEvent, GraphSagaEventEnvelope, GraphSagaInput,
    GraphSagaProjection, apply_graph_result, replay_graph, replay_graph_ordered, start_graph,
};
