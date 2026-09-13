//! Backend-neutral ports for Penelope's hexagonal architecture.
//!
//! This crate intentionally contains interfaces and versioned DTO contracts
//! only. Database, broker, scheduler, HTTP, StateChronicle-client, and worker
//! implementations belong in an outer composition root.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use async_trait::async_trait;
use penelope_domain::{
    ActionId, CanonicalCommandDtoV1, CanonicalEventDtoV1, LogicalTimeV1, ManualReviewDtoV1,
    PrincipalId, ProcessActionDtoV1, ProcessInputDtoV1, ProcessOutcomeDtoV1, ProcessScopeV1,
    ReviewId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A backend-independent port error.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PortError {
    /// The adapter could not currently perform its durable operation.
    #[error("port unavailable")]
    Unavailable,
    /// Optimistic concurrency rejected the expected durable process version.
    #[error("port optimistic concurrency conflict")]
    Conflict,
    /// The adapter denied the authenticated principal's requested operation.
    #[error("port operation is unauthorized")]
    Unauthorized,
    /// A configured bounded resource or rate quota was exhausted.
    #[error("port quota exceeded")]
    QuotaExceeded,
    /// The adapter could not complete the operation before its bounded deadline.
    #[error("port operation timed out")]
    TimedOut,
    /// The operation was durably cancelled before it could complete.
    #[error("port operation was cancelled")]
    Cancelled,
    /// The adapter cannot safely classify whether an external effect occurred.
    #[error("port external effect outcome is ambiguous")]
    Ambiguous,
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
    /// An outcome declares a schema other than the immutable outcome schema.
    #[error("atomic process commit outcome schema is invalid")]
    InvalidOutcomeSchema,
    /// One outcome is for a different pinned process-definition scope.
    #[error("atomic process commit outcome scope does not match")]
    OutcomeScopeMismatch,
    /// One outgoing action is for a different pinned process-definition scope.
    #[error("atomic process commit action scope does not match")]
    ActionScopeMismatch,
    /// The deduplicated input is for a different tenant or process.
    #[error("atomic process commit input scope does not match")]
    InputScopeMismatch,
    /// Advancing the expected outcome sequence would overflow.
    #[error("atomic process commit outcome sequence overflowed")]
    SequenceOverflow,
}

/// Typed validation failure for canonical-effect reconciliation evidence.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationValidationError {
    /// The evidence was returned for a different process action.
    #[error("canonical reconciliation action does not match the requested action")]
    ActionMismatch,
}

/// Authoritative result of reconciling a canonical external effect.
///
/// `Unknown` is deliberately distinct from `NotCommitted`: callers must not
/// retry an unknown effect merely because no success response was received.
/// An adapter may return `NotCommitted` only after consulting the authoritative
/// canonical evidence source according to its documented consistency window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanonicalReconciliationV1 {
    /// A verified immutable canonical event proves the effect committed.
    Committed {
        /// The committed canonical evidence.
        event: CanonicalEventDtoV1,
    },
    /// Authoritative evidence proves the action did not commit.
    NotCommitted {
        /// The action whose absence was authoritatively established.
        action_id: ActionId,
    },
    /// The effect cannot safely be classified as committed or absent.
    Unknown {
        /// The action requiring escalation or later reconciliation.
        action_id: ActionId,
    },
}

/// Typed resolution selected by an authorized manual-review operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManualReviewResolutionV1 {
    /// Authorize a retry only after durable evidence establishes it is safe.
    RetryAction,
    /// Authorize a compensating action under the pinned process definition.
    Compensate,
    /// Cancel the process without further automatic effects.
    Cancel,
    /// Keep the process escalated for a higher-authority decision.
    Escalate,
}

/// A process operation requiring a caller-specific authorization decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessAuthorizationOperationV1 {
    /// Start a new process instance.
    Start,
    /// Request process cancellation.
    Cancel,
    /// Request an explicit retry.
    Retry,
    /// Submit a manual-review decision.
    DecideReview,
}

/// Typed authorization request for one process operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessAuthorizationRequestV1 {
    /// Immutable process and definition scope targeted by the operation.
    pub scope: ProcessScopeV1,
    /// Authenticated caller identity.
    pub principal_id: PrincipalId,
    /// Typed requested operation.
    pub operation: ProcessAuthorizationOperationV1,
}

/// Fail-closed authorization result returned by an outer policy adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessAuthorizationDecisionV1 {
    /// The caller is authorized for the requested operation.
    Authorized,
    /// The caller is not authorized; no process mutation may occur.
    Denied,
}

/// Due-time record for one durable timer action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerScheduleV1 {
    /// Independently idempotent timer action to deliver through the inbox.
    pub action: ProcessActionDtoV1,
    /// Deterministic due time supplied by the application layer.
    pub due_at: LogicalTimeV1,
}

/// Idempotent claim request for a manual-review case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualReviewClaimV1 {
    /// The immutable review case being claimed.
    pub review_id: ReviewId,
    /// Validated identity of the claiming principal.
    pub claimed_by: PrincipalId,
}

/// Immutable, attributable manual-review resolution request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualReviewDecisionV1 {
    /// The immutable review case being decided.
    pub review_id: ReviewId,
    /// Validated identity of the authorized deciding principal.
    pub decided_by: PrincipalId,
    /// Typed process-safe resolution selected by the operator.
    pub resolution: ManualReviewResolutionV1,
    /// Digest of the redacted evidence and authorization record.
    pub evidence_digest: penelope_domain::ContentDigest,
}

impl CanonicalReconciliationV1 {
    /// Validates that reconciliation evidence belongs to the requested action.
    ///
    /// # Errors
    ///
    /// Returns a typed error if the result could be confused with evidence for
    /// another action.
    pub fn validate_for(
        &self,
        requested_action_id: &ActionId,
    ) -> Result<(), ReconciliationValidationError> {
        let observed_action_id = match self {
            Self::Committed { event } => &event.action_id,
            Self::NotCommitted { action_id } | Self::Unknown { action_id } => action_id,
        };
        if observed_action_id == requested_action_id {
            Ok(())
        } else {
            Err(ReconciliationValidationError::ActionMismatch)
        }
    }
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
    /// Returns a typed validation error when sequences or pinned process scopes
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

    /// Validates outcome order and a single pinned process-definition scope.
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
            if outcome.validate().is_err() {
                return Err(CommitValidationError::InvalidOutcomeSchema);
            }
            if outcome.sequence != expected_sequence {
                return Err(CommitValidationError::NonContiguousSequence);
            }
            if outcome.tenant_id != first_outcome.tenant_id
                || outcome.process_id != first_outcome.process_id
                || outcome.definition_id != first_outcome.definition_id
                || outcome.definition_version != first_outcome.definition_version
                || outcome.definition_digest != first_outcome.definition_digest
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
                || action.definition_id != first_outcome.definition_id
                || action.definition_version != first_outcome.definition_version
                || action.definition_digest != first_outcome.definition_digest
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
    async fn schedule(&self, timer: &TimerScheduleV1) -> Result<(), PortError>;
    /// Cancels a previously scheduled action idempotently.
    async fn cancel(&self, action_id: &ActionId) -> Result<(), PortError>;
}

/// Injected wall-clock boundary for deterministic application decisions.
#[async_trait]
pub trait Clock: Send + Sync {
    /// Returns the current time as a typed value; pure engine code never calls it.
    async fn now(&self) -> Result<LogicalTimeV1, PortError>;
}

/// Injected source of fresh independently idempotent action identities.
#[async_trait]
pub trait ActionIdSource: Send + Sync {
    /// Allocates an action identity for the supplied pinned process scope.
    async fn next_action_id(&self, scope: &ProcessScopeV1) -> Result<ActionId, PortError>;
}

/// Authorization boundary for mutable process operations.
#[async_trait]
pub trait ProcessAuthorizer: Send + Sync {
    /// Makes a fail-closed authorization decision for one typed request.
    async fn authorize(
        &self,
        request: &ProcessAuthorizationRequestV1,
    ) -> Result<ProcessAuthorizationDecisionV1, PortError>;
}

/// Canonical-state boundary implemented by a StateChronicle adapter.
#[async_trait]
pub trait CanonicalState: Send + Sync {
    /// Submits a command with the Penelope action ID as canonical idempotency ID.
    async fn submit(&self, command: &CanonicalCommandDtoV1) -> Result<(), PortError>;
    /// Reconciles a pending action from authoritative canonical evidence.
    ///
    /// `Unknown` must be escalated or reconciled again; it is not permission to
    /// resubmit a potentially non-idempotent remote effect.
    async fn reconcile(
        &self,
        action: &ProcessActionDtoV1,
    ) -> Result<CanonicalReconciliationV1, PortError>;
}

/// Durable manual-review escalation boundary.
#[async_trait]
pub trait ManualReviewQueue: Send + Sync {
    /// Opens or returns the idempotent review case.
    async fn open(&self, review: &ManualReviewDtoV1) -> Result<(), PortError>;
    /// Claims a review case without changing the process projection directly.
    async fn claim(&self, claim: &ManualReviewClaimV1) -> Result<(), PortError>;
    /// Records an authorized immutable resolution for subsequent inbox delivery.
    async fn decide(&self, decision: &ManualReviewDecisionV1) -> Result<(), PortError>;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use penelope_domain::{
        CanonicalCommitId, CanonicalEventDtoV1, CanonicalEventId, CausationIdV1, ContentDigest,
        LogicalTimeV1, OperationId, OutcomeActorV1, OutcomeId, ProcessActionKindV1,
        ProcessOutcomeFactV1, ProcessOutcomeKindV1, ProcessScopeV1, ResourceId, StepId, TenantId,
    };

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    fn outcome(sequence: u64) -> ProcessOutcomeDtoV1 {
        ProcessOutcomeDtoV1::new(
            ProcessScopeV1::new(
                id::<TenantId>("tnt_game"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            sequence,
            ProcessOutcomeFactV1::new(
                id::<OutcomeId>("out_event"),
                CausationIdV1::Action(id("act_cause")),
                OutcomeActorV1::System,
                LogicalTimeV1(1),
                ProcessOutcomeKindV1::ActionPlanned,
                ContentDigest([1; 32]),
            ),
        )
    }

    fn action() -> ProcessActionDtoV1 {
        ProcessActionDtoV1::new(
            ProcessScopeV1::new(
                id::<TenantId>("tnt_game"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
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

    #[test]
    fn atomic_commit_rejects_outgoing_action_from_another_definition() {
        let mut wrong_action = action();
        wrong_action.definition_digest = ContentDigest([8; 32]);
        let error =
            AtomicProcessCommitV1::new(0, None, vec![outcome(0)], vec![wrong_action]).unwrap_err();
        assert_eq!(error, CommitValidationError::ActionScopeMismatch);
    }

    #[test]
    fn atomic_commit_rejects_outcomes_from_different_definitions() {
        let mut wrong_outcome = outcome(1);
        wrong_outcome.definition_version = id("dfv_two");
        let error =
            AtomicProcessCommitV1::new(0, None, vec![outcome(0), wrong_outcome], vec![action()])
                .unwrap_err();
        assert_eq!(error, CommitValidationError::OutcomeScopeMismatch);
    }

    #[test]
    fn atomic_commit_rejects_an_outcome_with_another_schema() {
        let mut wrong_outcome = outcome(0);
        wrong_outcome.schema = penelope_domain::SchemaV1::ProcessAction;
        let error = AtomicProcessCommitV1::new(0, None, vec![wrong_outcome], vec![]).unwrap_err();
        assert_eq!(error, CommitValidationError::InvalidOutcomeSchema);
    }

    #[test]
    fn reconciliation_rejects_evidence_for_another_action() {
        let result = CanonicalReconciliationV1::Unknown {
            action_id: id("act_other"),
        };
        assert_eq!(
            result.validate_for(&id("act_dispatch")),
            Err(ReconciliationValidationError::ActionMismatch)
        );
    }

    #[test]
    fn reconciliation_rejects_committed_evidence_for_another_action() {
        let result = CanonicalReconciliationV1::Committed {
            event: CanonicalEventDtoV1 {
                tenant_id: id("tnt_game"),
                source_event_id: id::<CanonicalEventId>("cev_source"),
                action_id: id("act_other"),
                commit_id: id::<CanonicalCommitId>("cmt_commit"),
                commit_sequence: 0,
                operation: id::<OperationId>("op_settle"),
                resource_ids: vec![id::<ResourceId>("res_market")],
                payload_digest: ContentDigest([3; 32]),
            },
        };
        assert_eq!(
            result.validate_for(&id("act_dispatch")),
            Err(ReconciliationValidationError::ActionMismatch)
        );
    }
}
