//! Application-layer composition boundary for Penelope.
//!
//! The future executor will make deterministic decisions from versioned domain
//! DTOs and invoke only injected ports. It will not contain database, broker,
//! timer, HTTP, or StateChronicle client implementations.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

/// Pure deterministic saga decision engine.
pub mod engine;
