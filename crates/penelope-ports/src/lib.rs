//! Backend-neutral ports for Penelope's hexagonal architecture.
//!
//! This crate intentionally contains interfaces and versioned DTO contracts
//! only. Database, broker, scheduler, HTTP, StateChronicle-client, and worker
//! implementations belong in an outer composition root.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use async_trait::async_trait;
use penelope_domain::{
    ActionId, CanonicalCommandDtoV1, CanonicalEventDtoV1, CanonicalEventId, EffectKeyV1,
    ExternalReferenceId, LogicalTimeV1, ManualReviewDtoV1, OutcomeId, PrincipalId,
    ProcessActionDtoV1, ProcessInputDtoV1, ProcessInputKindV1, ProcessOutcomeDtoV1, ProcessScopeV1,
    ReviewId,
};
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU16, NonZeroU64};
use thiserror::Error;

/// Maximum append-only outcomes accepted in one atomic process commit.
pub const MAX_OUTCOMES_PER_ATOMIC_COMMIT: usize = 128;
/// Maximum independently idempotent actions accepted in one atomic commit.
pub const MAX_ACTIONS_PER_ATOMIC_COMMIT: usize = 128;
/// Maximum immutable outcomes returned by one bounded replay-read request.
pub const MAX_OUTCOMES_PER_READ_PAGE: u16 = 512;
/// Maximum redelivery attempts represented by one outbox record.
pub const MAX_OUTBOX_DELIVERY_ATTEMPTS: u32 = 64;
/// Maximum records returned by one durable outbox claim.
pub const MAX_OUTBOX_CLAIM_BATCH: u16 = 128;

/// Opaque fencing token assigned to one outbox lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxLeaseTokenV1(NonZeroU64);

impl OutboxLeaseTokenV1 {
    /// Creates a non-zero fencing token.
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }
}

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

/// Durable acknowledgement state of an independently idempotent outbox item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutboxAcknowledgementV1 {
    /// The action remains eligible for claiming or redelivery.
    Pending,
    /// The adapter recorded a successful dispatch acknowledgement.
    Acknowledged,
}

/// Versioned outbox record retained with the exact action and delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxRecordV1 {
    /// Independently idempotent action being delivered.
    pub action: ProcessActionDtoV1,
    /// Monotonic delivery attempt number for this action.
    pub delivery_attempt: u32,
    /// Explicit durable acknowledgement state.
    pub acknowledgement: OutboxAcknowledgementV1,
}

impl OutboxRecordV1 {
    /// Creates a pending outbox record at the first delivery attempt.
    #[must_use]
    pub const fn new(action: ProcessActionDtoV1) -> Self {
        Self {
            action,
            delivery_attempt: 0,
            acknowledgement: OutboxAcknowledgementV1::Pending,
        }
    }

    /// Validates the action schema and bounded delivery attempt.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invariant`] when the action or attempt is invalid.
    pub const fn validate(&self) -> Result<(), PortError> {
        if self.action.validate().is_err() || self.delivery_attempt >= MAX_OUTBOX_DELIVERY_ATTEMPTS
        {
            return Err(PortError::Invariant);
        }
        Ok(())
    }

    /// Returns an acknowledged copy; acknowledging an already acknowledged
    /// record is idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invariant`] when the record is invalid.
    pub fn acknowledge(mut self) -> Result<Self, PortError> {
        self.validate()?;
        self.acknowledgement = OutboxAcknowledgementV1::Acknowledged;
        Ok(self)
    }

    /// Advances one pending record to its next bounded delivery attempt.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invariant`] when the record is acknowledged or
    /// the attempt bound would be exceeded.
    pub fn next_delivery_attempt(mut self) -> Result<Self, PortError> {
        self.validate()?;
        if self.acknowledgement != OutboxAcknowledgementV1::Pending
            || self.delivery_attempt.saturating_add(1) >= MAX_OUTBOX_DELIVERY_ATTEMPTS
        {
            return Err(PortError::Invariant);
        }
        self.delivery_attempt = self.delivery_attempt.saturating_add(1);
        Ok(self)
    }
}

/// Bounded, scope-pinned request for claiming pending outbox records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxClaimRequestV1 {
    /// Process scope whose records may be claimed.
    pub scope: ProcessScopeV1,
    /// Maximum records to return.
    pub limit: NonZeroU16,
}

impl OutboxClaimRequestV1 {
    /// Creates a bounded outbox claim request.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::QuotaExceeded`] when `limit` exceeds the claim bound.
    pub fn new(scope: ProcessScopeV1, limit: NonZeroU16) -> Result<Self, PortError> {
        let request = Self { scope, limit };
        request.validate()?;
        Ok(request)
    }

    /// Validates the claim batch bound.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::QuotaExceeded`] when the limit exceeds the claim bound.
    pub const fn validate(&self) -> Result<(), PortError> {
        if self.limit.get() > MAX_OUTBOX_CLAIM_BATCH {
            Err(PortError::QuotaExceeded)
        } else {
            Ok(())
        }
    }
}

/// One claimed outbox record and its fencing lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxLeaseV1 {
    /// Claimed action and delivery state.
    pub record: OutboxRecordV1,
    /// Principal/worker that owns the lease.
    pub owner: PrincipalId,
    /// Opaque token used to fence stale acknowledgements.
    pub token: OutboxLeaseTokenV1,
    /// Inclusive logical lease expiry supplied by the adapter.
    pub lease_expires_at: LogicalTimeV1,
}

impl OutboxLeaseV1 {
    /// Validates the claimed record and lease identity.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invariant`] when the record is invalid.
    pub const fn validate(&self) -> Result<(), PortError> {
        self.record.validate()
    }

    /// Validates that this lease is for the exact requested process scope.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invariant`] when the lease action is cross-scoped.
    pub fn validate_for_claim(&self, request: &OutboxClaimRequestV1) -> Result<(), PortError> {
        self.validate()?;
        let action = &self.record.action;
        if action.tenant_id != request.scope.tenant_id
            || action.process_id != request.scope.process_id
            || action.definition_id != request.scope.definition_id
            || action.definition_version != request.scope.definition_version
            || action.definition_digest != request.scope.definition_digest
        {
            return Err(PortError::Invariant);
        }
        Ok(())
    }
}

/// Typed validation failure for one atomic process commit request.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CommitValidationError {
    /// A commit must contain at least one durable outcome.
    #[error("atomic process commit contains no outcomes")]
    EmptyOutcomes,
    /// The deduplicated input declares a schema other than the immutable input schema.
    #[error("atomic process commit input schema is invalid")]
    InvalidInputSchema,
    /// An outcome does not use the expected contiguous sequence.
    #[error("atomic process commit outcome sequence is not contiguous")]
    NonContiguousSequence,
    /// A commit attempted to append too many outcomes at once.
    #[error("atomic process commit exceeds the outcome limit")]
    OutcomeLimitExceeded,
    /// A commit contains the same immutable outcome identity more than once.
    #[error("atomic process commit contains a duplicate outcome identity")]
    DuplicateOutcomeId,
    /// An outcome declares a schema other than the immutable outcome schema.
    #[error("atomic process commit outcome schema is invalid")]
    InvalidOutcomeSchema,
    /// An outgoing action declares a schema other than the immutable action schema.
    #[error("atomic process commit action schema is invalid")]
    InvalidActionSchema,
    /// One outcome is for a different pinned process-definition scope.
    #[error("atomic process commit outcome scope does not match")]
    OutcomeScopeMismatch,
    /// One outgoing action is for a different pinned process-definition scope.
    #[error("atomic process commit action scope does not match")]
    ActionScopeMismatch,
    /// A commit attempted to enqueue too many actions at once.
    #[error("atomic process commit exceeds the action limit")]
    ActionLimitExceeded,
    /// A commit contains the same independently idempotent action identity twice.
    #[error("atomic process commit contains a duplicate action identity")]
    DuplicateActionId,
    /// A commit contains two action identities for the same semantic effect.
    #[error("atomic process commit contains a duplicate action effect key")]
    DuplicateActionEffectKey,
    /// The deduplicated input is for a different tenant or process.
    #[error("atomic process commit input scope does not match")]
    InputScopeMismatch,
    /// Canonical source-event deduplication requires an inbox input in the same commit.
    #[error("canonical source-event key requires an atomic inbox input")]
    CanonicalSourceWithoutInput,
    /// A canonical source-event key may only deduplicate a canonical-event input.
    #[error("canonical source-event key requires a canonical-event input")]
    CanonicalSourceWithWrongInputKind,
    /// An atomically accepted input must causally explain at least one appended outcome.
    #[error("atomic process commit input is not the cause of any appended outcome")]
    InputNotCausallyRecorded,
    /// Advancing the expected outcome sequence would overflow.
    #[error("atomic process commit outcome sequence overflowed")]
    SequenceOverflow,
}

/// Typed validation failure for a bounded immutable outcome replay page.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum OutcomePageValidationError {
    /// A requested page exceeds the contract's bounded outcome limit.
    #[error("outcome replay page exceeds the configured limit")]
    LimitExceeded,
    /// One returned outcome declared a schema other than the immutable outcome schema.
    #[error("outcome replay page contains an invalid outcome schema")]
    InvalidOutcomeSchema,
    /// One returned outcome does not match the page's pinned process scope.
    #[error("outcome replay page contains an outcome from another process scope")]
    OutcomeScopeMismatch,
    /// Returned outcomes do not begin at the requested sequence and remain contiguous.
    #[error("outcome replay page sequence is not contiguous")]
    NonContiguousSequence,
    /// The supplied continuation does not immediately follow the page's final outcome.
    #[error("outcome replay page continuation is inconsistent")]
    InvalidContinuation,
    /// Advancing a page sequence would overflow.
    #[error("outcome replay page sequence overflowed")]
    SequenceOverflow,
}

/// Typed validation failure for canonical-effect reconciliation evidence.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationValidationError {
    /// The evidence was returned for a different process action.
    #[error("canonical reconciliation action does not match the requested action")]
    ActionMismatch,
    /// The evidence was returned for another pinned process-definition scope.
    #[error("canonical reconciliation scope does not match the requested action")]
    ScopeMismatch,
    /// Committed evidence is malformed or has another schema identity.
    #[error("canonical reconciliation committed evidence is invalid")]
    InvalidCommittedEvidence,
    /// Committed evidence has a tenant different from the requested action.
    #[error("canonical reconciliation committed evidence tenant does not match")]
    TenantMismatch,
}

/// Typed validation failure for one immutable manual-review operation.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ManualReviewValidationError {
    /// The supplied review record does not carry the immutable review schema.
    #[error("manual review record schema is invalid")]
    InvalidReviewSchema,
    /// A claim or decision does not target the review's pinned process scope.
    #[error("manual review operation scope does not match the review")]
    ScopeMismatch,
    /// A claim or decision names another review case.
    #[error("manual review operation does not match the review identity")]
    ReviewIdMismatch,
    /// A dual-control decision was made by the same principal that claimed it.
    #[error("manual review dual control requires a distinct deciding principal")]
    DualControlViolation,
    /// A claim or decision occurred after the review's immutable deadline.
    #[error("manual review operation occurred after the review deadline")]
    Expired,
}

/// Typed validation failure for an external-effect dispatch or reconciliation record.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum EffectReconciliationValidationError {
    /// The request action does not carry the immutable action schema.
    #[error("external effect action schema is invalid")]
    InvalidActionSchema,
    /// Evidence belongs to another independently idempotent action.
    #[error("external effect evidence action does not match the request")]
    ActionMismatch,
    /// Evidence does not retain the action's exact semantic idempotency key.
    #[error("external effect evidence key does not match the request")]
    EffectKeyMismatch,
}

/// Typed validation failure for a durable retry-timer record.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum TimerValidationError {
    /// The timer action does not carry the immutable action schema.
    #[error("timer action schema is invalid")]
    InvalidActionSchema,
    /// A non-timer action was supplied to a timer scheduler boundary.
    #[error("timer schedule requires a timer action kind")]
    InvalidActionKind,
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
        /// Immutable process and definition scope of the reconciled action.
        scope: ProcessScopeV1,
        /// The committed canonical evidence.
        event: CanonicalEventDtoV1,
    },
    /// Authoritative evidence proves the action did not commit.
    NotCommitted {
        /// Immutable process and definition scope of the reconciled action.
        scope: ProcessScopeV1,
        /// The action whose absence was authoritatively established.
        action_id: ActionId,
    },
    /// The effect cannot safely be classified as committed or absent.
    Unknown {
        /// Immutable process and definition scope of the reconciled action.
        scope: ProcessScopeV1,
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

/// Required operator separation for one immutable manual-review decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManualReviewControlV1 {
    /// One authorized operator may claim and decide the review.
    SingleOperator,
    /// The deciding principal must differ from the principal that claimed it.
    DistinctDecider,
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

impl ProcessAuthorizationDecisionV1 {
    /// Returns whether the requested operation may proceed.
    #[must_use]
    pub const fn is_authorized(self) -> bool {
        matches!(self, Self::Authorized)
    }

    /// Converts this decision into a fail-closed port result.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Unauthorized`] for a denied decision.
    pub const fn require_authorized(self) -> Result<(), PortError> {
        match self {
            Self::Authorized => Ok(()),
            Self::Denied => Err(PortError::Unauthorized),
        }
    }
}

/// Typed external-effect execution or reconciliation result.
///
/// `Unknown` is distinct from `KnownFailure`: a timeout or response loss may
/// leave an external effect indeterminate, in which case callers must
/// reconcile again or escalate rather than blindly dispatch another effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExternalEffectStateV1 {
    /// Authoritative evidence proves the external effect completed.
    Succeeded,
    /// Authoritative evidence proves the external effect did not complete.
    KnownFailure,
    /// The external effect cannot safely be classified yet.
    Unknown,
}

/// Stable, idempotent request supplied to an external effect adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectDispatchRequestV1 {
    /// Independently idempotent action to execute or reconcile.
    pub action: ProcessActionDtoV1,
    /// Semantic effect key derived from the exact action coordinates.
    pub effect_key: EffectKeyV1,
    /// Optional inclusive logical deadline supplied by the application layer.
    pub deadline: Option<LogicalTimeV1>,
}

/// Durable evidence returned by an external effect adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalEffectEvidenceV1 {
    /// Independently idempotent action this evidence describes.
    pub action_id: ActionId,
    /// Exact semantic key of the action this evidence describes.
    pub effect_key: EffectKeyV1,
    /// Opaque remote reference if the external system assigned one.
    pub external_reference_id: Option<ExternalReferenceId>,
    /// Authoritative result classification.
    pub state: ExternalEffectStateV1,
}

impl EffectDispatchRequestV1 {
    /// Creates an external effect request with no application deadline.
    #[must_use]
    pub fn new(action: ProcessActionDtoV1) -> Self {
        let effect_key = action.effect_key();
        Self {
            action,
            effect_key,
            deadline: None,
        }
    }

    /// Attaches an inclusive logical execution/reconciliation deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: LogicalTimeV1) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Validates the immutable action schema and derived effect key.
    ///
    /// # Errors
    ///
    /// Returns a typed error when a caller substitutes an action or key.
    pub fn validate(&self) -> Result<(), EffectReconciliationValidationError> {
        if self.action.validate().is_err() {
            return Err(EffectReconciliationValidationError::InvalidActionSchema);
        }
        if self.effect_key != self.action.effect_key() {
            return Err(EffectReconciliationValidationError::EffectKeyMismatch);
        }
        Ok(())
    }
}

impl ExternalEffectEvidenceV1 {
    /// Validates that evidence belongs to the exact requested effect.
    ///
    /// # Errors
    ///
    /// Returns a typed error when action identity or semantic effect key differs.
    pub fn validate_for(
        &self,
        request: &EffectDispatchRequestV1,
    ) -> Result<(), EffectReconciliationValidationError> {
        request.validate()?;
        if self.action_id != request.action.action_id {
            return Err(EffectReconciliationValidationError::ActionMismatch);
        }
        if self.effect_key != request.effect_key {
            return Err(EffectReconciliationValidationError::EffectKeyMismatch);
        }
        Ok(())
    }
}

/// Due-time record for one durable timer action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerScheduleV1 {
    /// Independently idempotent timer action to deliver through the inbox.
    pub action: ProcessActionDtoV1,
    /// Deterministic due time supplied by the application layer.
    pub due_at: LogicalTimeV1,
}

impl TimerScheduleV1 {
    /// Validates that this record retains a full, typed timer action scope.
    ///
    /// # Errors
    ///
    /// Returns a typed error when an action schema or kind is substituted.
    pub fn validate(&self) -> Result<(), TimerValidationError> {
        if self.action.validate().is_err() {
            return Err(TimerValidationError::InvalidActionSchema);
        }
        if self.action.kind != penelope_domain::ProcessActionKindV1::Timer {
            return Err(TimerValidationError::InvalidActionKind);
        }
        Ok(())
    }
}

/// Idempotent claim request for a manual-review case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualReviewClaimV1 {
    /// Immutable process and definition scope of the review being claimed.
    pub scope: ProcessScopeV1,
    /// The immutable review case being claimed.
    pub review_id: ReviewId,
    /// Validated identity of the claiming principal.
    pub claimed_by: PrincipalId,
    /// Logical time at which the durable claim was recorded.
    pub claimed_at: LogicalTimeV1,
}

/// Immutable, attributable manual-review resolution request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualReviewDecisionV1 {
    /// Immutable process and definition scope of the review being decided.
    pub scope: ProcessScopeV1,
    /// The immutable review case being decided.
    pub review_id: ReviewId,
    /// Principal that holds the durable claim being resolved.
    pub claimed_by: PrincipalId,
    /// Validated identity of the authorized deciding principal.
    pub decided_by: PrincipalId,
    /// Logical time at which the durable decision was recorded.
    pub decided_at: LogicalTimeV1,
    /// Typed process-safe resolution selected by the operator.
    pub resolution: ManualReviewResolutionV1,
    /// Required separation between the claim and decision principals.
    pub control: ManualReviewControlV1,
    /// Digest of the redacted evidence and authorization record.
    pub evidence_digest: penelope_domain::ContentDigest,
}

impl ManualReviewClaimV1 {
    /// Validates that this claim is bound to the immutable review it targets.
    ///
    /// # Errors
    ///
    /// Returns a typed error if the review identity or full process-definition
    /// scope differs.
    pub fn validate_for(
        &self,
        review: &ManualReviewDtoV1,
    ) -> Result<(), ManualReviewValidationError> {
        if review.validate().is_err() {
            return Err(ManualReviewValidationError::InvalidReviewSchema);
        }
        if self.review_id != review.review_id {
            return Err(ManualReviewValidationError::ReviewIdMismatch);
        }
        if self.scope != review.scope() {
            return Err(ManualReviewValidationError::ScopeMismatch);
        }
        if review
            .expires_at
            .is_some_and(|expires_at| self.claimed_at > expires_at)
        {
            return Err(ManualReviewValidationError::Expired);
        }
        Ok(())
    }
}

impl ManualReviewDecisionV1 {
    /// Validates review binding and any declared dual-control requirement.
    ///
    /// # Errors
    ///
    /// Returns a typed error if review identity/scope differs or the selected
    /// control policy is violated.
    pub fn validate_for(
        &self,
        review: &ManualReviewDtoV1,
    ) -> Result<(), ManualReviewValidationError> {
        if review.validate().is_err() {
            return Err(ManualReviewValidationError::InvalidReviewSchema);
        }
        if self.review_id != review.review_id {
            return Err(ManualReviewValidationError::ReviewIdMismatch);
        }
        if self.scope != review.scope() {
            return Err(ManualReviewValidationError::ScopeMismatch);
        }
        if review
            .expires_at
            .is_some_and(|expires_at| self.decided_at > expires_at)
        {
            return Err(ManualReviewValidationError::Expired);
        }
        if matches!(self.control, ManualReviewControlV1::DistinctDecider)
            && self.claimed_by == self.decided_by
        {
            return Err(ManualReviewValidationError::DualControlViolation);
        }
        Ok(())
    }
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
            Self::Committed { event, .. } => &event.action_id,
            Self::NotCommitted { action_id, .. } | Self::Unknown { action_id, .. } => action_id,
        };
        if observed_action_id == requested_action_id {
            Ok(())
        } else {
            Err(ReconciliationValidationError::ActionMismatch)
        }
    }

    /// Validates that reconciliation evidence is pinned to the complete action
    /// scope and, for committed evidence, is a valid same-tenant event.
    ///
    /// # Errors
    ///
    /// Returns a typed error when scope, action identity, event schema, or
    /// tenant would permit evidence for another process to advance this action.
    pub fn validate_for_action(
        &self,
        action: &ProcessActionDtoV1,
    ) -> Result<(), ReconciliationValidationError> {
        let scope = match self {
            Self::Committed { scope, .. }
            | Self::NotCommitted { scope, .. }
            | Self::Unknown { scope, .. } => scope,
        };
        if scope != &action.scope() {
            return Err(ReconciliationValidationError::ScopeMismatch);
        }
        self.validate_for(&action.action_id)?;
        if let Self::Committed { event, .. } = self {
            if event.validate().is_err() {
                return Err(ReconciliationValidationError::InvalidCommittedEvidence);
            }
            if event.tenant_id != action.tenant_id {
                return Err(ReconciliationValidationError::TenantMismatch);
            }
        }
        Ok(())
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
    /// Verified canonical source event deduplicated with this input, when this
    /// commit advances from a StateChronicle committed-event delivery.
    ///
    /// An adapter must set this only after `penelope-statechronicle` verifies
    /// the event. The store must deduplicate it in the same transaction as
    /// `input`, outcomes, and actions, preventing the same source event from
    /// advancing the process under a second inbox identity.
    #[serde(default)]
    pub canonical_source_event_id: Option<CanonicalEventId>,
    /// Contiguous append-only process outcomes.
    pub outcomes: Vec<ProcessOutcomeDtoV1>,
    /// Independently idempotent outgoing actions persisted with the outcomes.
    pub actions: Vec<ProcessActionDtoV1>,
}

/// A bounded request to read immutable outcomes for deterministic replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeReplayRequestV1 {
    /// Immutable process and definition scope being replayed.
    pub scope: ProcessScopeV1,
    /// First per-process outcome sequence requested.
    pub from_sequence: u64,
    /// Maximum records accepted in this page.
    pub limit: NonZeroU16,
}

impl OutcomeReplayRequestV1 {
    /// Creates a bounded immutable outcome replay request.
    ///
    /// # Errors
    ///
    /// Returns a typed error when `limit` exceeds the public page bound.
    pub fn new(
        scope: ProcessScopeV1,
        from_sequence: u64,
        limit: NonZeroU16,
    ) -> Result<Self, OutcomePageValidationError> {
        let request = Self {
            scope,
            from_sequence,
            limit,
        };
        request.validate()?;
        Ok(request)
    }

    /// Validates the bounded replay page size.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the requested page is too large.
    pub const fn validate(&self) -> Result<(), OutcomePageValidationError> {
        if self.limit.get() > MAX_OUTCOMES_PER_READ_PAGE {
            Err(OutcomePageValidationError::LimitExceeded)
        } else {
            Ok(())
        }
    }
}

/// One bounded immutable outcome page returned for deterministic replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeReplayPageV1 {
    /// Immutable process and definition scope shared by every returned outcome.
    pub scope: ProcessScopeV1,
    /// First per-process outcome sequence returned in this page.
    pub from_sequence: u64,
    /// Ordered immutable outcomes, bounded by [`MAX_OUTCOMES_PER_READ_PAGE`].
    pub outcomes: Vec<ProcessOutcomeDtoV1>,
    /// Immediate continuation when more immutable outcomes exist.
    pub next_sequence: Option<u64>,
}

impl OutcomeReplayPageV1 {
    /// Creates and validates a bounded replay page.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid schemas, scopes, ordering, bounds, or
    /// continuation semantics.
    pub fn new(
        scope: ProcessScopeV1,
        from_sequence: u64,
        outcomes: Vec<ProcessOutcomeDtoV1>,
        next_sequence: Option<u64>,
    ) -> Result<Self, OutcomePageValidationError> {
        let page = Self {
            scope,
            from_sequence,
            outcomes,
            next_sequence,
        };
        page.validate()?;
        Ok(page)
    }

    /// Validates exact scope, ordering, bounded size, and continuation.
    ///
    /// # Errors
    ///
    /// Returns a typed error when this page cannot safely drive deterministic replay.
    pub fn validate(&self) -> Result<(), OutcomePageValidationError> {
        if self.outcomes.len() > usize::from(MAX_OUTCOMES_PER_READ_PAGE) {
            return Err(OutcomePageValidationError::LimitExceeded);
        }
        let mut expected_sequence = self.from_sequence;
        for outcome in &self.outcomes {
            if outcome.validate().is_err() {
                return Err(OutcomePageValidationError::InvalidOutcomeSchema);
            }
            if outcome.tenant_id != self.scope.tenant_id
                || outcome.process_id != self.scope.process_id
                || outcome.definition_id != self.scope.definition_id
                || outcome.definition_version != self.scope.definition_version
                || outcome.definition_digest != self.scope.definition_digest
            {
                return Err(OutcomePageValidationError::OutcomeScopeMismatch);
            }
            if outcome.sequence != expected_sequence {
                return Err(OutcomePageValidationError::NonContiguousSequence);
            }
            expected_sequence = expected_sequence
                .checked_add(1)
                .ok_or(OutcomePageValidationError::SequenceOverflow)?;
        }
        if self.outcomes.is_empty() && self.next_sequence.is_some() {
            return Err(OutcomePageValidationError::InvalidContinuation);
        }
        if self
            .next_sequence
            .is_some_and(|next| next != expected_sequence)
        {
            return Err(OutcomePageValidationError::InvalidContinuation);
        }
        Ok(())
    }
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
            canonical_source_event_id: None,
            outcomes,
            actions,
        };
        commit.validate()?;
        Ok(commit)
    }

    /// Binds this commit to a previously verified canonical source event.
    #[must_use]
    pub fn with_canonical_source_event(mut self, source_event_id: CanonicalEventId) -> Self {
        self.canonical_source_event_id = Some(source_event_id);
        self
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
        if self.outcomes.len() > MAX_OUTCOMES_PER_ATOMIC_COMMIT {
            return Err(CommitValidationError::OutcomeLimitExceeded);
        }
        let mut expected_sequence = self.expected_sequence;
        let expected_scope = first_outcome.scope();
        for (index, outcome) in self.outcomes.iter().enumerate() {
            if outcome.validate().is_err() {
                return Err(CommitValidationError::InvalidOutcomeSchema);
            }
            if outcome.validate_for_scope(&expected_scope).is_err() {
                return Err(CommitValidationError::OutcomeScopeMismatch);
            }
            if outcome.sequence != expected_sequence {
                return Err(CommitValidationError::NonContiguousSequence);
            }
            if self
                .outcomes
                .iter()
                .skip(index.saturating_add(1))
                .any(|other_outcome| other_outcome.outcome_id == outcome.outcome_id)
            {
                return Err(CommitValidationError::DuplicateOutcomeId);
            }
            expected_sequence = expected_sequence
                .checked_add(1)
                .ok_or(CommitValidationError::SequenceOverflow)?;
        }
        if self.actions.len() > MAX_ACTIONS_PER_ATOMIC_COMMIT {
            return Err(CommitValidationError::ActionLimitExceeded);
        }
        for (index, action) in self.actions.iter().enumerate() {
            if action.validate().is_err() {
                return Err(CommitValidationError::InvalidActionSchema);
            }
            if action.tenant_id != first_outcome.tenant_id
                || action.process_id != first_outcome.process_id
                || action.definition_id != first_outcome.definition_id
                || action.definition_version != first_outcome.definition_version
                || action.definition_digest != first_outcome.definition_digest
            {
                return Err(CommitValidationError::ActionScopeMismatch);
            }
            if self
                .actions
                .iter()
                .skip(index.saturating_add(1))
                .any(|other_action| other_action.action_id == action.action_id)
            {
                return Err(CommitValidationError::DuplicateActionId);
            }
            let effect_key = action.effect_key();
            if self
                .actions
                .iter()
                .skip(index.saturating_add(1))
                .any(|other_action| other_action.effect_key() == effect_key)
            {
                return Err(CommitValidationError::DuplicateActionEffectKey);
            }
        }
        if let Some(input) = &self.input {
            if input.validate().is_err() {
                return Err(CommitValidationError::InvalidInputSchema);
            }
            if input.tenant_id != first_outcome.tenant_id
                || input.process_id != first_outcome.process_id
            {
                return Err(CommitValidationError::InputScopeMismatch);
            }
        }
        if self.canonical_source_event_id.is_some() {
            let Some(input) = &self.input else {
                return Err(CommitValidationError::CanonicalSourceWithoutInput);
            };
            if input.kind != ProcessInputKindV1::CanonicalEvent {
                return Err(CommitValidationError::CanonicalSourceWithWrongInputKind);
            }
        }
        if let Some(input) = &self.input
            && !self.outcomes.iter().any(|outcome| {
                matches!(&outcome.causation_id, penelope_domain::CausationIdV1::Input(input_id) if input_id == &input.input_id)
            })
        {
            return Err(CommitValidationError::InputNotCausallyRecorded);
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
        scope: &ProcessScopeV1,
        expected_sequence: u64,
        outcomes: &[ProcessOutcomeDtoV1],
    ) -> Result<(), PortError>;

    /// Reads one bounded immutable outcome page for deterministic replay.
    async fn read_outcomes(
        &self,
        request: &OutcomeReplayRequestV1,
    ) -> Result<OutcomeReplayPageV1, PortError>;
}

/// Backend-neutral durable outbox claim and acknowledgement boundary.
///
/// Implementations must claim records atomically with a lease/fencing policy,
/// return only records for the requested process scope, and make
/// acknowledgement idempotent by action identity and effect key.
#[async_trait]
pub trait OutboxStore: Send + Sync {
    /// Claims a bounded batch of pending records for one process scope.
    async fn claim(&self, request: &OutboxClaimRequestV1) -> Result<Vec<OutboxLeaseV1>, PortError>;

    /// Acknowledges one exact action delivery after successful dispatch.
    async fn acknowledge(&self, lease: &OutboxLeaseV1) -> Result<(), PortError>;
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

/// External-effect execution boundary with explicit reconciliation and cancellation.
///
/// Implementations must persist or otherwise retain the exact action ID and
/// effect key before sending an effect. `Unknown` evidence is not permission
/// to retry. Cancellation is best effort and cannot erase an already-issued
/// external effect; callers must reconcile its final state afterward.
#[async_trait]
pub trait ExternalEffectExecutor: Send + Sync {
    /// Attempts the independently idempotent external effect.
    async fn execute(
        &self,
        request: &EffectDispatchRequestV1,
    ) -> Result<ExternalEffectEvidenceV1, PortError>;
    /// Resolves a potentially ambiguous external effect by its stable identity.
    async fn reconcile(
        &self,
        request: &EffectDispatchRequestV1,
    ) -> Result<ExternalEffectEvidenceV1, PortError>;
    /// Requests best-effort cancellation without assuming the effect is absent.
    async fn cancel(&self, request: &EffectDispatchRequestV1) -> Result<(), PortError>;
}

/// Durable timer scheduling boundary.
#[async_trait]
pub trait TimerScheduler: Send + Sync {
    /// Schedules a timer action whose firing returns through the inbox.
    async fn schedule(&self, timer: &TimerScheduleV1) -> Result<(), PortError>;
    /// Cancels a timer while retaining its full pinned action scope.
    async fn cancel(&self, timer: &TimerScheduleV1) -> Result<(), PortError>;
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

/// Injected source of immutable outcome identities.
///
/// The application must allocate every identity before constructing an atomic
/// outcome commit. Adapters must never derive an outcome identity from a clock,
/// a sequence number, or an untyped string.
#[async_trait]
pub trait OutcomeIdSource: Send + Sync {
    /// Allocates an outcome identity for the supplied pinned process scope.
    async fn next_outcome_id(&self, scope: &ProcessScopeV1) -> Result<OutcomeId, PortError>;
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

    fn outcome(sequence: u64, outcome_id: OutcomeId) -> ProcessOutcomeDtoV1 {
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
                outcome_id,
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

    fn input() -> ProcessInputDtoV1 {
        ProcessInputDtoV1::new(
            id("tnt_game"),
            id("prc_trade"),
            id("inp_event"),
            penelope_domain::ProcessInputKindV1::CanonicalEvent,
            ContentDigest([3; 32]),
        )
    }

    #[test]
    fn atomic_commit_requires_contiguous_outcomes_in_one_scope() {
        let commit =
            AtomicProcessCommitV1::new(0, None, vec![outcome(0, id("out_event"))], vec![action()]);
        assert!(commit.is_ok());
    }

    #[test]
    fn atomic_commit_rejects_outgoing_action_from_another_process() {
        let mut wrong_action = action();
        wrong_action.process_id = id("prc_other");
        let error = AtomicProcessCommitV1::new(
            0,
            None,
            vec![outcome(0, id("out_event"))],
            vec![wrong_action],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::ActionScopeMismatch);
    }

    #[test]
    fn atomic_commit_rejects_outgoing_action_from_another_definition() {
        let mut wrong_action = action();
        wrong_action.definition_digest = ContentDigest([8; 32]);
        let error = AtomicProcessCommitV1::new(
            0,
            None,
            vec![outcome(0, id("out_event"))],
            vec![wrong_action],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::ActionScopeMismatch);
    }

    #[test]
    fn atomic_commit_rejects_outcomes_from_different_definitions() {
        let mut wrong_outcome = outcome(1, id("out_second"));
        wrong_outcome.definition_version = id("dfv_two");
        let error = AtomicProcessCommitV1::new(
            0,
            None,
            vec![outcome(0, id("out_first")), wrong_outcome],
            vec![action()],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::OutcomeScopeMismatch);
    }

    #[test]
    fn atomic_commit_rejects_an_outcome_with_another_schema() {
        let mut wrong_outcome = outcome(0, id("out_event"));
        wrong_outcome.schema = penelope_domain::SchemaV1::ProcessAction;
        let error = AtomicProcessCommitV1::new(0, None, vec![wrong_outcome], vec![]).unwrap_err();
        assert_eq!(error, CommitValidationError::InvalidOutcomeSchema);
    }

    #[test]
    fn atomic_commit_rejects_an_action_with_another_schema() {
        let mut wrong_action = action();
        wrong_action.schema = penelope_domain::SchemaV1::ProcessOutcome;
        let error = AtomicProcessCommitV1::new(
            0,
            None,
            vec![outcome(0, id("out_event"))],
            vec![wrong_action],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::InvalidActionSchema);
    }

    #[test]
    fn atomic_commit_rejects_an_input_with_another_schema() {
        let mut wrong_input = input();
        wrong_input.schema = penelope_domain::SchemaV1::ProcessAction;
        let error = AtomicProcessCommitV1::new(
            0,
            Some(wrong_input),
            vec![outcome(0, id("out_event"))],
            vec![action()],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::InvalidInputSchema);
    }

    #[test]
    fn authorization_decisions_fail_closed_without_message_matching() {
        assert!(ProcessAuthorizationDecisionV1::Authorized.is_authorized());
        assert_eq!(
            ProcessAuthorizationDecisionV1::Authorized.require_authorized(),
            Ok(())
        );
        assert!(!ProcessAuthorizationDecisionV1::Denied.is_authorized());
        assert_eq!(
            ProcessAuthorizationDecisionV1::Denied.require_authorized(),
            Err(PortError::Unauthorized)
        );
    }

    #[test]
    fn outbox_record_requires_valid_action_and_bounded_attempt() {
        let mut record = OutboxRecordV1::new(action());
        assert!(record.validate().is_ok());
        record = record.next_delivery_attempt().unwrap();
        assert_eq!(record.delivery_attempt, 1);
        record = record.acknowledge().unwrap();
        assert_eq!(
            record.clone().next_delivery_attempt(),
            Err(PortError::Invariant)
        );
        assert_eq!(
            record.clone().acknowledge().unwrap().acknowledgement,
            OutboxAcknowledgementV1::Acknowledged
        );
        record.delivery_attempt = MAX_OUTBOX_DELIVERY_ATTEMPTS;
        assert_eq!(record.validate(), Err(PortError::Invariant));
    }

    #[test]
    fn outbox_lease_rejects_a_cross_scope_claim() {
        let request =
            OutboxClaimRequestV1::new(outcome(0, id("out_claim_scope")).scope(), NonZeroU16::MIN)
                .unwrap();
        let mut action = action();
        action.process_id = id("prc_other");
        let lease = OutboxLeaseV1 {
            record: OutboxRecordV1::new(action),
            owner: id("pri_worker"),
            token: OutboxLeaseTokenV1::new(NonZeroU64::MIN),
            lease_expires_at: LogicalTimeV1(5),
        };
        assert_eq!(
            lease.validate_for_claim(&request),
            Err(PortError::Invariant)
        );
    }

    #[test]
    fn outbox_claim_request_rejects_an_oversized_batch() {
        let request = OutboxClaimRequestV1 {
            scope: outcome(0, id("out_claim_scope")).scope(),
            limit: NonZeroU16::new(MAX_OUTBOX_CLAIM_BATCH.saturating_add(1)).unwrap(),
        };
        assert_eq!(request.validate(), Err(PortError::QuotaExceeded));
    }

    #[test]
    fn canonical_source_event_key_requires_a_canonical_inbox_input() {
        let source_event_id: CanonicalEventId = id("cev_lock");
        let mut accepted_outcome = outcome(0, id("out_event"));
        accepted_outcome.causation_id = penelope_domain::CausationIdV1::Input(id("inp_event"));
        let valid =
            AtomicProcessCommitV1::new(0, Some(input()), vec![accepted_outcome], vec![action()])
                .unwrap()
                .with_canonical_source_event(source_event_id.clone());
        assert_eq!(valid.validate(), Ok(()));

        let mut missing_input =
            AtomicProcessCommitV1::new(0, None, vec![outcome(0, id("out_event"))], vec![action()])
                .unwrap();
        missing_input.canonical_source_event_id = Some(source_event_id.clone());
        assert_eq!(
            missing_input.validate(),
            Err(CommitValidationError::CanonicalSourceWithoutInput)
        );

        let mut wrong_kind = AtomicProcessCommitV1::new(
            0,
            Some(input()),
            vec![{
                let mut accepted = outcome(0, id("out_event"));
                accepted.causation_id = penelope_domain::CausationIdV1::Input(id("inp_event"));
                accepted
            }],
            vec![action()],
        )
        .unwrap();
        wrong_kind.input.as_mut().unwrap().kind = ProcessInputKindV1::TimerFired;
        wrong_kind.canonical_source_event_id = Some(source_event_id);
        assert_eq!(
            wrong_kind.validate(),
            Err(CommitValidationError::CanonicalSourceWithWrongInputKind)
        );
    }

    #[test]
    fn atomic_commit_requires_accepted_input_causation() {
        let error = AtomicProcessCommitV1::new(
            0,
            Some(input()),
            vec![outcome(0, id("out_event"))],
            vec![action()],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::InputNotCausallyRecorded);
    }

    #[test]
    fn external_effect_evidence_requires_the_exact_action_and_effect_key() {
        let request = EffectDispatchRequestV1::new(action()).with_deadline(LogicalTimeV1(10));
        assert_eq!(request.validate(), Ok(()));
        let evidence = ExternalEffectEvidenceV1 {
            action_id: request.action.action_id.clone(),
            effect_key: request.effect_key.clone(),
            external_reference_id: Some(id("ext_remote_effect")),
            state: ExternalEffectStateV1::Unknown,
        };
        assert_eq!(evidence.validate_for(&request), Ok(()));

        let wrong_action = ExternalEffectEvidenceV1 {
            action_id: id("act_other"),
            ..evidence
        };
        assert_eq!(
            wrong_action.validate_for(&request),
            Err(EffectReconciliationValidationError::ActionMismatch)
        );

        let mut mismatched_request = request;
        mismatched_request.effect_key.payload_digest = ContentDigest([8; 32]);
        assert_eq!(
            mismatched_request.validate(),
            Err(EffectReconciliationValidationError::EffectKeyMismatch)
        );
    }

    #[test]
    fn timer_schedule_requires_a_schema_valid_timer_action() {
        let schedule = TimerScheduleV1 {
            action: action(),
            due_at: LogicalTimeV1(10),
        };
        assert_eq!(
            schedule.validate(),
            Err(TimerValidationError::InvalidActionKind)
        );

        let mut timer_action = action();
        timer_action.kind = penelope_domain::ProcessActionKindV1::Timer;
        let timer_schedule = TimerScheduleV1 {
            action: timer_action,
            due_at: LogicalTimeV1(10),
        };
        assert_eq!(timer_schedule.validate(), Ok(()));

        let mut malformed_timer = timer_schedule;
        malformed_timer.action.schema = penelope_domain::SchemaV1::ProcessOutcome;
        assert_eq!(
            malformed_timer.validate(),
            Err(TimerValidationError::InvalidActionSchema)
        );
    }

    #[test]
    fn atomic_commit_rejects_duplicate_outcome_identity() {
        let error = AtomicProcessCommitV1::new(
            0,
            None,
            vec![
                outcome(0, id("out_duplicate")),
                outcome(1, id("out_duplicate")),
            ],
            vec![action()],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::DuplicateOutcomeId);
    }

    #[test]
    fn atomic_commit_rejects_an_oversized_outcome_batch() {
        let outcomes =
            vec![outcome(0, id("out_limit")); MAX_OUTCOMES_PER_ATOMIC_COMMIT.saturating_add(1)];
        let error = AtomicProcessCommitV1::new(0, None, outcomes, vec![]).unwrap_err();
        assert_eq!(error, CommitValidationError::OutcomeLimitExceeded);
    }

    #[test]
    fn atomic_commit_rejects_duplicate_action_identity() {
        let error = AtomicProcessCommitV1::new(
            0,
            None,
            vec![outcome(0, id("out_event"))],
            vec![action(), action()],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::DuplicateActionId);
    }

    #[test]
    fn atomic_commit_rejects_duplicate_action_effect_key() {
        let mut second_action = action();
        second_action.action_id = id("act_other");
        let error = AtomicProcessCommitV1::new(
            0,
            None,
            vec![outcome(0, id("out_event"))],
            vec![action(), second_action],
        )
        .unwrap_err();
        assert_eq!(error, CommitValidationError::DuplicateActionEffectKey);
    }

    #[test]
    fn atomic_commit_rejects_an_oversized_action_batch() {
        let actions = vec![action(); MAX_ACTIONS_PER_ATOMIC_COMMIT.saturating_add(1)];
        let error = AtomicProcessCommitV1::new(0, None, vec![outcome(0, id("out_event"))], actions)
            .unwrap_err();
        assert_eq!(error, CommitValidationError::ActionLimitExceeded);
    }

    #[test]
    fn outcome_replay_pages_are_bounded_scope_pinned_and_contiguous() {
        let page = OutcomeReplayPageV1::new(
            ProcessScopeV1::new(
                id("tnt_game"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            0,
            vec![outcome(0, id("out_first")), outcome(1, id("out_second"))],
            Some(2),
        );
        assert!(page.is_ok());

        let bad_continuation = OutcomeReplayPageV1::new(
            ProcessScopeV1::new(
                id("tnt_game"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            0,
            vec![outcome(0, id("out_first"))],
            Some(2),
        )
        .unwrap_err();
        assert_eq!(
            bad_continuation,
            OutcomePageValidationError::InvalidContinuation
        );

        let empty_continuation = OutcomeReplayPageV1::new(
            ProcessScopeV1::new(
                id("tnt_game"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            0,
            vec![],
            Some(0),
        )
        .unwrap_err();
        assert_eq!(
            empty_continuation,
            OutcomePageValidationError::InvalidContinuation
        );

        let oversized_limit = OutcomeReplayRequestV1::new(
            ProcessScopeV1::new(
                id("tnt_game"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            0,
            std::num::NonZeroU16::new(MAX_OUTCOMES_PER_READ_PAGE.saturating_add(1)).unwrap(),
        )
        .unwrap_err();
        assert_eq!(oversized_limit, OutcomePageValidationError::LimitExceeded);
    }

    #[test]
    fn reconciliation_rejects_evidence_for_another_action() {
        let result = CanonicalReconciliationV1::Unknown {
            scope: action().scope(),
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
            scope: action().scope(),
            event: CanonicalEventDtoV1::new(
                id("tnt_game"),
                id::<CanonicalEventId>("cev_source"),
                id("act_other"),
                id::<CanonicalCommitId>("cmt_commit"),
                0,
                id::<OperationId>("op_settle"),
                vec![id::<ResourceId>("res_market")],
                ContentDigest([3; 32]),
            )
            .unwrap(),
        };
        assert_eq!(
            result.validate_for(&id("act_dispatch")),
            Err(ReconciliationValidationError::ActionMismatch)
        );
    }

    #[test]
    fn reconciliation_requires_the_action_scope_and_valid_committed_evidence() {
        let expected = action();
        let valid = CanonicalReconciliationV1::NotCommitted {
            scope: expected.scope(),
            action_id: expected.action_id.clone(),
        };
        assert_eq!(valid.validate_for_action(&expected), Ok(()));

        let wrong_scope = CanonicalReconciliationV1::Unknown {
            scope: ProcessScopeV1::new(
                id("tnt_game"),
                id("prc_other"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            action_id: expected.action_id.clone(),
        };
        assert_eq!(
            wrong_scope.validate_for_action(&expected),
            Err(ReconciliationValidationError::ScopeMismatch)
        );
    }

    #[test]
    fn manual_review_operations_require_the_pinned_scope_and_dual_control() {
        let review = ManualReviewDtoV1::new(
            ProcessScopeV1::new(
                id("tnt_game"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            id("rev_trade"),
            4,
            Some(LogicalTimeV1(10)),
            ContentDigest([6; 32]),
        );
        let claim = ManualReviewClaimV1 {
            scope: review.scope(),
            review_id: review.review_id.clone(),
            claimed_by: id("pri_claimant"),
            claimed_at: LogicalTimeV1(5),
        };
        assert_eq!(claim.validate_for(&review), Ok(()));

        let valid_decision = ManualReviewDecisionV1 {
            scope: review.scope(),
            review_id: review.review_id.clone(),
            claimed_by: claim.claimed_by.clone(),
            decided_by: id("pri_decider"),
            decided_at: LogicalTimeV1(10),
            resolution: ManualReviewResolutionV1::Compensate,
            control: ManualReviewControlV1::DistinctDecider,
            evidence_digest: ContentDigest([7; 32]),
        };
        assert_eq!(valid_decision.validate_for(&review), Ok(()));

        let mut malformed_review = review.clone();
        malformed_review.schema = penelope_domain::SchemaV1::ProcessAction;
        assert_eq!(
            claim.validate_for(&malformed_review),
            Err(ManualReviewValidationError::InvalidReviewSchema)
        );
        assert_eq!(
            valid_decision.validate_for(&malformed_review),
            Err(ManualReviewValidationError::InvalidReviewSchema)
        );

        let same_operator = ManualReviewDecisionV1 {
            decided_by: claim.claimed_by.clone(),
            ..valid_decision
        };
        assert_eq!(
            same_operator.validate_for(&review),
            Err(ManualReviewValidationError::DualControlViolation)
        );

        let wrong_scope = ManualReviewClaimV1 {
            scope: ProcessScopeV1::new(
                id("tnt_game"),
                id("prc_other"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            ..claim
        };
        assert_eq!(
            wrong_scope.validate_for(&review),
            Err(ManualReviewValidationError::ScopeMismatch)
        );

        let expired = ManualReviewDecisionV1 {
            decided_at: LogicalTimeV1(11),
            ..same_operator
        };
        assert_eq!(
            expired.validate_for(&review),
            Err(ManualReviewValidationError::Expired)
        );
    }
}
