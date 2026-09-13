//! Penelope umbrella crate.
//!
//! This is the consumer-facing facade for Penelope's hexagonal architecture:
//! pure protocol primitives, versioned DTOs, intent boundary, deterministic
//! executor, backend-neutral ports, and adapter boundaries. It ships no
//! database, broker, scheduler, HTTP, or StateChronicle client implementation.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

/// Pure protocol primitives and schema identifiers.
pub use penelope_core as core;
/// Versioned data-transfer objects.
pub use penelope_domain as domain;
/// Application execution boundary.
pub use penelope_executor as executor;
/// Inbound parsing and validation boundary.
pub use penelope_intent as intent;
/// Backend-neutral ports implemented by the composition root.
pub use penelope_ports as ports;
/// StateChronicle adapter boundary; no client implementation is included.
pub use penelope_statechronicle as statechronicle;
