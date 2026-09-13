//! Backend-neutral ports for Penelope's hexagonal architecture.
//!
//! This crate intentionally contains interfaces and versioned DTO contracts
//! only. Database, broker, scheduler, HTTP, StateChronicle-client, and worker
//! implementations belong in an outer composition root.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use async_trait::async_trait;
use penelope_domain::{
    CanonicalCommandDtoV1, CanonicalEventDtoV1, ManualReviewDtoV1, ProcessActionDtoV1,
    ProcessInputDtoV1, ProcessOutcomeDtoV1,
};
use thiserror::Error;

/// A backend-independent port error.
#[derive(Debug, Error)]
pub enum PortError {
    /// The adapter could not currently perform its durable operation.
    #[error("port unavailable: {0}")]
    Unavailable(String),
    /// The adapter rejected a versioned DTO or port invariant.
    #[error("port invariant violation: {0}")]
    Invariant(String),
}

/// Durable append-only process outcome store.
#[async_trait]
pub trait ProcessStore: Send + Sync {
    /// Appends outcomes using the expected per-instance sequence.
    async fn append_outcomes(
        &self,
        expected_sequence: u64,
        outcomes: &[ProcessOutcomeDtoV1],
    ) -> Result<(), PortError>;
}

/// Durable inbox that deduplicates immutable source inputs.
#[async_trait]
pub trait Inbox: Send + Sync {
    /// Records an input exactly once before it is processed.
    async fn accept(&self, input: &ProcessInputDtoV1) -> Result<(), PortError>;
}

/// Durable process-action dispatch boundary.
#[async_trait]
pub trait ActionDispatcher: Send + Sync {
    /// Dispatches one action using its stable action identity as idempotency key.
    async fn dispatch(&self, action: &ProcessActionDtoV1) -> Result<(), PortError>;
}

/// Durable timer scheduling boundary.
#[async_trait]
pub trait TimerScheduler: Send + Sync {
    /// Schedules a timer action whose firing returns through the inbox.
    async fn schedule(&self, action: &ProcessActionDtoV1) -> Result<(), PortError>;
    /// Cancels a previously scheduled action idempotently.
    async fn cancel(&self, action_id: &str) -> Result<(), PortError>;
}

/// Canonical-state boundary implemented by a StateChronicle adapter.
#[async_trait]
pub trait CanonicalState: Send + Sync {
    /// Submits a command with the Penelope action ID as canonical idempotency ID.
    async fn submit(&self, command: &CanonicalCommandDtoV1) -> Result<(), PortError>;
    /// Reconciles a pending action from durable canonical evidence.
    async fn reconcile(
        &self,
        action: &ProcessActionDtoV1,
    ) -> Result<Option<CanonicalEventDtoV1>, PortError>;
}

/// Durable manual-review escalation boundary.
#[async_trait]
pub trait ManualReviewQueue: Send + Sync {
    /// Opens or returns the idempotent review case.
    async fn open(&self, review: &ManualReviewDtoV1) -> Result<(), PortError>;
}
