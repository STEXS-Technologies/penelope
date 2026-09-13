//! Penelope — the process-truth brain.
//!
//! An independent open-source saga/process orchestration engine (pure logic,
//! no I/O). It owns PROCESS truth — long-running workflows that cannot be one
//! atomic transaction: waits, timers, retries, external side effects, and
//! planned compensation.
//!
//! A process instance is an **append-only log of step outcomes**; its current
//! state is a deterministic projection of that log:
//!
//! ```text
//! state(instance) = f(def_version, ordered outcomes)
//! ```
//!
//! The engine's transition function is pure: `actions = apply(input, def,
//! outcome_log)`. Side effects are *recorded, never recomputed*; timers are
//! first-class events that enter the log and are replayed as data. The brain
//! is 100% unit-testable with fake outcomes and fake timer events.
//!
//! Penelope never writes canonical state directly — it consumes committed
//! events and submits commands/intents for any new canonical change (the
//! intents-in / events-out boundary).

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

/// Versioned, immutable saga definitions (schema `penelope.process.def.v0`).
pub mod definition;

/// Process instances: append-only outcome log + deterministic projection.
pub mod instance;

/// Step outcomes and the outcome log (schema `penelope.process.outcome.v0`).
pub mod outcome;

/// LIFO compensation planning, exactly-once guards, and escalation rules.
pub mod compensation;

/// Process inputs and the actions the engine emits.
pub mod decision;

/// Process engine error type.
pub mod error;
