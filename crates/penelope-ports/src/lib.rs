//! Penelope ports — the adapter surface for the process engine.
//!
//! Backend-agnostic port traits only (no implementations in this crate):
//! `Sync + Send`, dyn-compatible async traits, per-port `thiserror` error
//! enums, and doc comments on every public item. Production adapters live in
//! the consuming platform's composition root.
//!
//! The ports define *where* process state lives and *how* steps execute; the
//! `penelope` brain crate defines *what* the process does.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

/// Durable process-instance store (load, append outcome + projection).
pub mod process_store;

/// Executes a saga step and records its outcome (idempotent by key).
pub mod step_executor;

/// Durable timer scheduling (waits, deadlines, retry backoff).
pub mod timer_scheduler;

/// Publishes process events (outbox → platform event stream).
pub mod event_sink;

/// Submits commands to a canonical state system (idempotent on command id).
pub mod intent_submitter;

/// Escalation queue for manual resolution (genericized review pattern).
pub mod manual_review_queue;

/// Event consumption with dedup keyed on `(tenant_id, event_id)`.
pub mod inbox;
