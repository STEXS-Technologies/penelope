//! Backend-neutral ports for Penelope's hexagonal architecture.
//!
//! This crate intentionally contains interfaces and versioned DTO contracts
//! only. Database, broker, scheduler, HTTP, StateChronicle-client, and worker
//! implementations belong in an outer composition root.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use async_trait::async_trait;
use penelope_domain::{
    ActionId, CanonicalCommandDtoV1, CanonicalEventDtoV1, ManualReviewDtoV1, ProcessActionDtoV1,
    ProcessInputDtoV1, ProcessOutcomeDtoV1,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A backend-independent port error.
#[derive(Debug, Error)]
pub enum PortError {
    /// The adapter could not currently perform its durable operation.
    #[error("port unavailable")]
    Unavailable,
    /// The adapter rejected a versioned DTO or port invariant.
    #[error("port invariant violation")]
    Invariant,
}

/// Typed validation failure for one atomic process commit request.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CommitValidationError {
    /// A commit must contain at least one durable outcome.
    #[error("atomic process commit contains no outcomes")]
    EmptyOutcomes,
    /// An outcome does not use the expected contiguous sequence.
    #[error("atomic process commit outcome sequence is not contiguous")]
    NonContiguousSequence,
    /// One outcome is for a different tenant or process.
    #[error("atomic process commit outcome scope does not match")]
    OutcomeScopeMismatch,
    /// One outgoing action is for a different tenant or process.
    #[error("atomic process commit action scope does not match")]
    ActionScopeMismatch,
    /// The deduplicated input is for a different tenant or process.
    #[error("atomic process commit input scope does not match")]
    InputScopeMismatch,
    /// Advancing the expected outcome sequence would overflow.
    #[error("atomic process commit outcome sequence overflowed")]
    SequenceOverflow,
}

/// Atomically persisted input, outcomes, and outgoing actions for one process.
///
/// An adapter must either make every field durable in one local transaction or
/// make none of them durable. `input`, when present, is the immutable inbox key
/// that is deduplicated in that same transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomicProcessCommitV1 {
    /// Expected first outcome sequence for optimistic concurrency.
    pub expected_sequence: u64,
    /// Optional immutable input accepted by the durable inbox.
    pub input: Option<ProcessInputDtoV1>,
    /// Contiguous append-only process outcomes.
    pub outcomes: Vec<ProcessOutcomeDtoV1>,
    /// Independently idempotent outgoing actions persisted with the outcomes.
    pub actions: Vec<ProcessActionDtoV1>,
}

impl AtomicProcessCommitV1 {
    /// Creates and validates a single atomic process commit request.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when sequences or tenant/process scopes
    /// are inconsistent.
    pub fn new(
        expected_sequence: u64,
        input: Option<ProcessInputDtoV1>,
        outcomes: Vec<ProcessOutcomeDtoV1>,
        actions: Vec<ProcessActionDtoV1>,
    ) -> Result<Self, CommitValidationError> {
        let commit = Self {
            expected_sequence,
            input,
            outcomes,
            actions,
        };
        commit.validate()?;
        Ok(commit)
    }

    /// Validates outcome order and a single tenant/process scope.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when this request cannot be committed
    /// atomically for one process.
    pub fn validate(&self) -> Result<(), CommitValidationError> {
        let Some(first_outcome) = self.outcomes.first() else {
            return Err(CommitValidationError::EmptyOutcomes);
        };
        let mut expected_sequence = self.expected_sequence;
        for outcome in &self.outcomes {
            if outcome.sequence != expected_sequence {
                return Err(CommitValidationError::NonContiguousSequence);
            }
            if outcome.tenant_id != first_outcome.tenant_id
                || outcome.process_id != first_outcome.process_id
            {
                return Err(CommitValidationError::OutcomeScopeMismatch);
            }
            expected_sequence = expected_sequence
                .checked_add(1)
                .ok_or(CommitValidationError::SequenceOverflow)?;
        }
        for action in &self.actions {
            if action.tenant_id != first_outcome.tenant_id
                || action.process_id != first_outcome.process_id
            {
                return Err(CommitValidationError::ActionScopeMismatch);
            }
        }
        if let Some(input) = &self.input
            && (input.tenant_id != first_outcome.tenant_id
                || input.process_id != first_outcome.process_id)
        {
            return Err(CommitValidationError::InputScopeMismatch);
        }
        Ok(())
    }
}

/// Receipt returned after a successful atomic local commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomicProcessCommitReceiptV1 {
    /// Last sequence made durable by this commit.
    pub committed_through_sequence: u64,
    /// Whether the inbox input was already durably accepted.
    pub duplicate_input: bool,
}

/// Durable append-only process outcome store.
#[async_trait]
pub trait ProcessStore: Send + Sync {
    /// Atomically commits inbox acceptance, outcomes, and outgoing actions.
    async fn commit(
        &self,
        commit: &AtomicProcessCommitV1,
    ) -> Result<AtomicProcessCommitReceiptV1, PortError>;

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
    async fn cancel(&self, action_id: &ActionId) -> Result<(), PortError>;
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use penelope_domain::{
        CausationIdV1, ContentDigest, OutcomeId, ProcessActionKindV1, ProcessOutcomeKindV1, StepId,
        TenantId,
    };

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    fn outcome(sequence: u64) -> ProcessOutcomeDtoV1 {
        ProcessOutcomeDtoV1::new(
            id::<TenantId>("tnt_game"),
            id("prc_trade"),
            sequence,
            id::<OutcomeId>("out_event"),
            CausationIdV1::Action(id("act_cause")),
            ProcessOutcomeKindV1::ActionPlanned,
            ContentDigest([1; 32]),
        )
    }

    fn action() -> ProcessActionDtoV1 {
        ProcessActionDtoV1::new(
            id::<TenantId>("tnt_game"),
            id("prc_trade"),
            id("act_dispatch"),
            id::<StepId>("stp_dispatch"),
            0,
            ProcessActionKindV1::CanonicalCommand,
            ContentDigest([2; 32]),
        )
    }

    #[test]
    fn atomic_commit_requires_contiguous_outcomes_in_one_scope() {
        let commit = AtomicProcessCommitV1::new(0, None, vec![outcome(0)], vec![action()]);
        assert!(commit.is_ok());
    }

    #[test]
    fn atomic_commit_rejects_outgoing_action_from_another_process() {
        let mut wrong_action = action();
        wrong_action.process_id = id("prc_other");
        let error =
            AtomicProcessCommitV1::new(0, None, vec![outcome(0)], vec![wrong_action]).unwrap_err();
        assert_eq!(error, CommitValidationError::ActionScopeMismatch);
    }
}
