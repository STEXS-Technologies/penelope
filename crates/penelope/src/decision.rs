//! Process inputs and the actions the engine emits.
//!
//! Inputs: correlation events (via inbox), timer firings, manual resolutions.
//! Actions: execute step, submit intent, schedule/cancel timer, compensate,
//! emit process event, escalate, complete.
