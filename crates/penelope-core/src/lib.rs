//! Pure Penelope protocol primitives.
//!
//! This crate is intentionally free of I/O, runtime, database, broker, and
//! StateChronicle dependencies. It owns only stable schema identifiers shared
//! by versioned DTOs and adapter contracts.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

/// Schema identifiers for the versioned public DTO boundary.
pub mod schema {
    /// Immutable process-definition DTO schema.
    pub const PROCESS_DEFINITION_V1: &str = "penelope.process.definition.v1";
    /// Process input DTO schema.
    pub const PROCESS_INPUT_V1: &str = "penelope.process.input.v1";
    /// Append-only process outcome DTO schema.
    pub const PROCESS_OUTCOME_V1: &str = "penelope.process.outcome.v1";
    /// Durable planned-action DTO schema.
    pub const PROCESS_ACTION_V1: &str = "penelope.process.action.v1";
    /// Canonical command DTO schema.
    pub const CANONICAL_COMMAND_V1: &str = "penelope.canonical.command.v1";
    /// Verified canonical-event DTO schema.
    pub const CANONICAL_EVENT_V1: &str = "penelope.canonical.event.v1";
    /// Manual-review DTO schema.
    pub const MANUAL_REVIEW_V1: &str = "penelope.manual-review.v1";
}
