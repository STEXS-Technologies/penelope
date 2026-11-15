//! Frozen 0.4 consumer imports. Keep this fixture independent of the workspace.
//! Additional behavior/replay fixtures are required before compatibility qualification.

pub use penelope::core::task::{TaskProjection, TaskRecord};
pub use penelope::domain::{ProcessAction, ProcessInput, ProcessOutcome};
pub use penelope::executor::{engine, graph};
pub use penelope::intent::parse_process_input;
pub use penelope::ports::{OutboxStore, OutcomeLog};
pub use penelope::statechronicle::VerifiedCanonicalEvent;
pub use penelope::task::TaskStatus;
pub use penelope::task::control::TaskControl;
pub use penelope::{
    ActionResult, AtomicProcessCommit, GraphSagaProjection, LinearSagaProjection,
    ProcessGraphDefinition, TaskSupervisor, apply_action_result, apply_graph_result, decide,
    replay, replay_graph, start, start_graph,
};

#[cfg(feature = "timeline")]
pub use penelope::timeline::{LiveTimeline, RewindableComputation};
