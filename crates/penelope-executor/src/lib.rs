#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

#[path = "mod.rs"]
mod entry;

pub use entry::*;

/// Recoverable task supervision, independent of the saga engines.
pub mod task;

/// Durable saga-to-task dispatch composition through injected ports.
pub mod composition;

mod graph_outcomes;

/// Bounded authorized integration calls through existing ports.
pub mod services;

/// Saga candidates inspected through the common prepared-decision view.
pub mod saga_machine;

/// Source-authorized machine handoff into native task admission.
pub mod machine_task;
