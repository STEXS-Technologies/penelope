//! Backend-neutral ports for Penelope's hexagonal architecture.
//!
//! This crate intentionally contains interfaces and versioned DTO contracts
//! only. Database, broker, scheduler, HTTP, StateChronicle-client, and worker
//! implementations belong in an outer composition root.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use async_trait::async_trait;
use penelope_domain::{
    ActionId, CanonicalCommandDtoV1, CanonicalEventDtoV1, CanonicalEventId, CanonicalWireBytesV1,
    DefinitionId, DefinitionMigrationV1, DefinitionVersion, EffectKeyV1, ExternalReferenceId,
    InputId, LogicalTimeV1, ManualReviewDtoV1, OutcomeId, PrincipalId, ProcessActionDtoV1,
    ProcessDefinitionDtoV1, ProcessInputDtoV1, ProcessInputKindV1, ProcessOutcomeDtoV1,
    ProcessScopeV1, ReviewId,
};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};
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
/// Maximum due timers returned by one worker claim.
pub const MAX_TIMER_CLAIM_BATCH: u16 = 128;
/// Maximum immutable outcomes retained by one in-memory replay accumulator.
/// Durable adapters may page larger histories through [`OutcomeReplayPageV1`].
pub const MAX_OUTCOMES_PER_REPLAY_LOG: usize = 4096;

/// Typed key for looking up one immutable registered definition version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionLookupV1 {
    /// Stable definition identity.
    pub definition_id: DefinitionId,
    /// Exact version requested by a process start or replay.
    pub definition_version: DefinitionVersion,
}

/// Receipt from idempotent immutable-definition registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionRegistrationReceiptV1 {
    /// Registered immutable definition identity.
    pub definition_id: DefinitionId,
    /// Registered immutable version.
    pub definition_version: DefinitionVersion,
    /// Digest acknowledged by the registry.
    pub definition_digest: penelope_domain::ContentDigest,
    /// Whether this identity and digest were already registered.
    pub duplicate: bool,
}

/// Receipt from idempotent definition-migration registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionMigrationReceiptV1 {
    /// Immutable migration identity acknowledged by the registry.
    pub migration_id: penelope_domain::MigrationId,
    /// Whether this migration identity was already registered.
    pub duplicate: bool,
}

/// Typed validation failure for a migration-registration receipt.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionMigrationReceiptValidationError {
    /// The registry acknowledged another migration identity.
    #[error("definition migration receipt does not match the requested migration")]
    MigrationMismatch,
}

impl DefinitionMigrationReceiptV1 {
    /// Creates a receipt for one immutable migration identity.
    #[must_use]
    pub const fn new(migration_id: penelope_domain::MigrationId, duplicate: bool) -> Self {
        Self {
            migration_id,
            duplicate,
        }
    }

    /// Requires this receipt to acknowledge the exact migration.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionMigrationReceiptValidationError::MigrationMismatch`]
    /// when the migration identity differs.
    pub fn validate_for(
        &self,
        migration: &DefinitionMigrationV1,
    ) -> Result<(), DefinitionMigrationReceiptValidationError> {
        if self.migration_id == migration.migration_id {
            Ok(())
        } else {
            Err(DefinitionMigrationReceiptValidationError::MigrationMismatch)
        }
    }
}

/// Typed validation failure for a definition-registration receipt.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionRegistrationReceiptValidationError {
    /// The registry acknowledged another immutable identity or digest.
    #[error("definition registration receipt does not match the requested definition")]
    DefinitionMismatch,
}

impl DefinitionRegistrationReceiptV1 {
    /// Creates a receipt for one immutable definition identity and digest.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(
        definition_id: DefinitionId,
        definition_version: DefinitionVersion,
        definition_digest: penelope_domain::ContentDigest,
        duplicate: bool,
    ) -> Self {
        Self {
            definition_id,
            definition_version,
            definition_digest,
            duplicate,
        }
    }

    /// Requires this receipt to acknowledge the exact definition record.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionRegistrationReceiptValidationError::DefinitionMismatch`]
    /// when identity, version, or digest differs.
    pub fn validate_for(
        &self,
        definition: &ProcessDefinitionDtoV1,
    ) -> Result<(), DefinitionRegistrationReceiptValidationError> {
        if self.definition_id == definition.definition_id
            && self.definition_version == definition.definition_version
            && self.definition_digest == definition.definition_digest
        {
            Ok(())
        } else {
            Err(DefinitionRegistrationReceiptValidationError::DefinitionMismatch)
        }
    }
}

/// Receipt from an idempotent durable inbox acceptance attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxAcceptanceReceiptV1 {
    /// Tenant scope of the accepted input.
    pub tenant_id: penelope_domain::TenantId,
    /// Process scope of the accepted input.
    pub process_id: penelope_domain::ProcessId,
    /// Immutable input identity accepted or redelivered.
    pub input_id: InputId,
    /// Whether this identity was already durably accepted.
    pub duplicate: bool,
}

/// Receipt from an idempotent manual-review mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManualReviewOperationV1 {
    /// Opening or returning an existing review case.
    Open,
    /// Claiming an existing review case.
    Claim,
    /// Recording a review decision.
    Decide,
}

/// Receipt from an idempotent manual-review mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualReviewReceiptV1 {
    /// Immutable review identity mutated or redelivered.
    pub review_id: ReviewId,
    /// Mutation operation acknowledged by the queue.
    pub operation: ManualReviewOperationV1,
    /// Whether the mutation was already durably recorded.
    pub duplicate: bool,
}

/// Receipt from an idempotent action-dispatch attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionDispatchReceiptV1 {
    /// Complete process-definition scope acknowledged by the dispatcher.
    pub scope: ProcessScopeV1,
    /// Immutable action identity dispatched or redelivered.
    pub action_id: ActionId,
    /// Whether this action was already durably dispatched.
    pub duplicate: bool,
}

/// Receipt from idempotent canonical-command submission/enqueue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalSubmitReceiptV1 {
    /// Complete process-definition scope acknowledged by the canonical adapter.
    pub tenant_id: penelope_domain::TenantId,
    /// Penelope action identity reused as the canonical idempotency key.
    pub action_id: ActionId,
    /// Whether this command identity was already durably submitted.
    pub duplicate: bool,
}

/// Typed validation failure for a canonical-submit receipt.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalSubmitReceiptValidationError {
    /// The adapter acknowledged another command identity.
    #[error("canonical submit receipt does not match the requested command")]
    ActionMismatch,
    /// The adapter acknowledged a command under another pinned scope.
    #[error("canonical submit receipt scope does not match the requested command")]
    ScopeMismatch,
}

impl CanonicalSubmitReceiptV1 {
    /// Creates a receipt for one canonical command identity.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(tenant_id: penelope_domain::TenantId, action_id: ActionId, duplicate: bool) -> Self {
        Self {
            tenant_id,
            action_id,
            duplicate,
        }
    }

    /// Requires this receipt to acknowledge the exact command action ID.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalSubmitReceiptValidationError::ActionMismatch`] when
    /// the adapter acknowledges another command.
    pub fn validate_for(
        &self,
        command: &CanonicalCommandDtoV1,
    ) -> Result<(), CanonicalSubmitReceiptValidationError> {
        if self.tenant_id != command.tenant_id {
            Err(CanonicalSubmitReceiptValidationError::ScopeMismatch)
        } else if self.action_id == command.action_id {
            Ok(())
        } else {
            Err(CanonicalSubmitReceiptValidationError::ActionMismatch)
        }
    }
}

/// Typed validation failure for an action-dispatch receipt.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ActionReceiptValidationError {
    /// The adapter returned a receipt for another action.
    #[error("action dispatch receipt does not match the requested action")]
    ActionMismatch,
    /// The adapter acknowledged an action under another pinned scope.
    #[error("action dispatch receipt scope does not match the requested action")]
    ScopeMismatch,
}

impl ActionDispatchReceiptV1 {
    /// Creates a receipt for one action identity.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(scope: ProcessScopeV1, action_id: ActionId, duplicate: bool) -> Self {
        Self {
            scope,
            action_id,
            duplicate,
        }
    }

    /// Requires this receipt to acknowledge the exact action identity.
    ///
    /// # Errors
    ///
    /// Returns [`ActionReceiptValidationError::ActionMismatch`] when the
    /// adapter acknowledges another action.
    pub fn validate_for(
        &self,
        action: &ProcessActionDtoV1,
    ) -> Result<(), ActionReceiptValidationError> {
        if self.scope != action.scope() {
            Err(ActionReceiptValidationError::ScopeMismatch)
        } else if self.action_id == action.action_id {
            Ok(())
        } else {
            Err(ActionReceiptValidationError::ActionMismatch)
        }
    }
}

/// Typed validation failure for a manual-review receipt.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ManualReviewReceiptValidationError {
    /// The adapter returned a receipt for another review case.
    #[error("manual-review receipt does not match the requested review")]
    ReviewMismatch,
    /// The adapter returned a receipt for another mutation operation.
    #[error("manual-review receipt operation does not match the requested operation")]
    OperationMismatch,
}

impl ManualReviewReceiptV1 {
    /// Creates a receipt for one review identity.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(review_id: ReviewId, duplicate: bool) -> Self {
        Self::new_for_operation(review_id, ManualReviewOperationV1::Open, duplicate)
    }

    /// Creates a receipt for one review identity and mutation operation.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn new_for_operation(
        review_id: ReviewId,
        operation: ManualReviewOperationV1,
        duplicate: bool,
    ) -> Self {
        Self {
            review_id,
            operation,
            duplicate,
        }
    }

    /// Requires this receipt to acknowledge the exact review identity.
    ///
    /// # Errors
    ///
    /// Returns [`ManualReviewReceiptValidationError::ReviewMismatch`] when
    /// the adapter acknowledges another case.
    pub fn validate_for(
        &self,
        review_id: &ReviewId,
    ) -> Result<(), ManualReviewReceiptValidationError> {
        if &self.review_id == review_id {
            Ok(())
        } else {
            Err(ManualReviewReceiptValidationError::ReviewMismatch)
        }
    }

    /// Requires this receipt to acknowledge the exact review mutation.
    ///
    /// # Errors
    ///
    /// Returns a typed error when either the review identity or operation
    /// differs from the requested mutation.
    pub fn validate_for_operation(
        &self,
        review_id: &ReviewId,
        operation: ManualReviewOperationV1,
    ) -> Result<(), ManualReviewReceiptValidationError> {
        self.validate_for(review_id)?;
        if self.operation == operation {
            Ok(())
        } else {
            Err(ManualReviewReceiptValidationError::OperationMismatch)
        }
    }
}

/// Typed validation failure for an inbox acceptance receipt.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum InboxReceiptValidationError {
    /// The adapter acknowledged a different immutable input identity.
    #[error("inbox acceptance receipt input does not match the submitted input")]
    InputMismatch,
    /// The adapter acknowledged another tenant or process.
    #[error("inbox acceptance receipt scope does not match the submitted input")]
    ScopeMismatch,
}

impl InboxAcceptanceReceiptV1 {
    /// Creates a receipt for one input identity.
    #[must_use]
    pub const fn new(
        tenant_id: penelope_domain::TenantId,
        process_id: penelope_domain::ProcessId,
        input_id: InputId,
        duplicate: bool,
    ) -> Self {
        Self {
            tenant_id,
            process_id,
            input_id,
            duplicate,
        }
    }

    /// Requires this receipt to acknowledge the exact submitted input.
    ///
    /// # Errors
    ///
    /// Returns [`InboxReceiptValidationError::InputMismatch`] when an adapter
    /// returns a receipt for another input identity.
    pub fn validate_for(
        &self,
        input: &ProcessInputDtoV1,
    ) -> Result<(), InboxReceiptValidationError> {
        if self.tenant_id != input.tenant_id || self.process_id != input.process_id {
            Err(InboxReceiptValidationError::ScopeMismatch)
        } else if self.input_id == input.input_id {
            Ok(())
        } else {
            Err(InboxReceiptValidationError::InputMismatch)
        }
    }
}

impl DefinitionLookupV1 {
    /// Creates a typed immutable-definition lookup key.
    #[must_use]
    pub const fn new(definition_id: DefinitionId, definition_version: DefinitionVersion) -> Self {
        Self {
            definition_id,
            definition_version,
        }
    }
}

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

/// Coarse, stable diagnostic class safe to expose without payload contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticClassV1 {
    /// Input or DTO validation failed.
    Validation,
    /// Authorization policy denied the operation.
    Authorization,
    /// Optimistic concurrency or deduplication conflict occurred.
    Conflict,
    /// The outer port was temporarily unavailable.
    Availability,
    /// A logical deadline or operation timeout was reached.
    Timeout,
    /// An external effect cannot yet be classified safely.
    Ambiguous,
    /// A caller or adapter violated a protocol invariant.
    Invariant,
}

impl DiagnosticClassV1 {
    /// Maps a port failure to a stable, non-sensitive class.
    #[must_use]
    pub const fn from_port_error(error: PortError) -> Self {
        match error {
            PortError::Unavailable | PortError::Cancelled => Self::Availability,
            PortError::Conflict => Self::Conflict,
            PortError::Unauthorized => Self::Authorization,
            PortError::QuotaExceeded | PortError::Invariant => Self::Invariant,
            PortError::TimedOut => Self::Timeout,
            PortError::Ambiguous => Self::Ambiguous,
        }
    }
}

/// Maximum diagnostic metadata size accepted from an adapter.
pub const MAX_REDACTED_DIAGNOSTIC_BYTES: u32 = 4096;
/// Maximum configured quota capacity represented by one request.
pub const MAX_QUOTA_CAPACITY: u32 = 1_000_000;

/// Stable resource class for bounded admission control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuotaKindV1 {
    /// Number of concurrently open process instances.
    OpenProcesses,
    /// Number of outstanding actions.
    OutstandingActions,
    /// Number of delivery attempts in a bounded interval.
    DeliveryAttempts,
    /// Number of outcomes accepted in a bounded interval.
    OutcomeWrites,
}

/// Typed quota admission request; no resource names or free-form policy text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaRequestV1 {
    /// Resource class being accounted.
    pub kind: QuotaKindV1,
    /// Units requested by the operation.
    pub requested: NonZeroU32,
    /// Maximum units permitted by the pinned policy.
    pub capacity: NonZeroU32,
}

/// Typed quota validation failure.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum QuotaValidationError {
    /// Configured capacity exceeds the global safety bound.
    #[error("quota capacity exceeds the configured bound")]
    CapacityLimitExceeded,
    /// Operation requests more units than the configured capacity.
    #[error("quota request exceeds configured capacity")]
    RequestExceedsCapacity,
}

impl QuotaRequestV1 {
    /// Creates and validates a bounded quota request.
    ///
    /// # Errors
    ///
    /// Returns a typed error when capacity or requested units exceed policy.
    pub const fn new(
        kind: QuotaKindV1,
        requested: NonZeroU32,
        capacity: NonZeroU32,
    ) -> Result<Self, QuotaValidationError> {
        let request = Self {
            kind,
            requested,
            capacity,
        };
        match request.validate() {
            Ok(()) => Ok(request),
            Err(error) => Err(error),
        }
    }

    /// Validates the global capacity and per-operation admission bound.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the request cannot be admitted safely.
    pub const fn validate(self) -> Result<(), QuotaValidationError> {
        if self.capacity.get() > MAX_QUOTA_CAPACITY {
            return Err(QuotaValidationError::CapacityLimitExceeded);
        }
        if self.requested.get() > self.capacity.get() {
            return Err(QuotaValidationError::RequestExceedsCapacity);
        }
        Ok(())
    }
}

/// Typed failure for constructing a bounded redacted diagnostic.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticValidationError {
    /// Adapter supplied an unbounded metadata-size claim.
    #[error("redacted diagnostic metadata exceeds the configured bound")]
    MetadataLimitExceeded,
}

/// Safe diagnostic metadata that deliberately contains no raw error message
/// or payload. `evidence_digest` refers to separately retained, access-
/// controlled evidence and is not itself a disclosure of that evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedDiagnosticV1 {
    /// Pinned process scope associated with the diagnostic.
    pub scope: ProcessScopeV1,
    /// Coarse stable classification.
    pub class: DiagnosticClassV1,
    /// Digest of access-controlled evidence, if any.
    pub evidence_digest: penelope_domain::ContentDigest,
    /// Bounded byte count of redacted metadata retained out of band.
    pub metadata_bytes: u32,
}

impl RedactedDiagnosticV1 {
    /// Creates bounded diagnostic metadata without accepting a raw message.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticValidationError::MetadataLimitExceeded`] when the
    /// claimed metadata size exceeds the public bound.
    pub fn new(
        scope: ProcessScopeV1,
        class: DiagnosticClassV1,
        evidence_digest: penelope_domain::ContentDigest,
        metadata_bytes: u32,
    ) -> Result<Self, DiagnosticValidationError> {
        if metadata_bytes > MAX_REDACTED_DIAGNOSTIC_BYTES {
            return Err(DiagnosticValidationError::MetadataLimitExceeded);
        }
        Ok(Self {
            scope,
            class,
            evidence_digest,
            metadata_bytes,
        })
    }
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
    /// Authenticated worker that will own returned leases.
    pub owner: PrincipalId,
    /// Maximum records to return.
    pub limit: NonZeroU16,
}

impl OutboxClaimRequestV1 {
    /// Creates a bounded outbox claim request.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::QuotaExceeded`] when `limit` exceeds the claim bound.
    pub fn new(
        scope: ProcessScopeV1,
        owner: PrincipalId,
        limit: NonZeroU16,
    ) -> Result<Self, PortError> {
        let request = Self {
            scope,
            owner,
            limit,
        };
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
        request.validate()?;
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
        if self.owner != request.owner {
            return Err(PortError::Invariant);
        }
        Ok(())
    }

    /// Validates scope, owner, record invariants, and lease expiry together.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::TimedOut`] for an expired lease or
    /// [`PortError::Invariant`] for a malformed or cross-scoped lease.
    pub fn validate_for_claim_at(
        &self,
        request: &OutboxClaimRequestV1,
        now: LogicalTimeV1,
    ) -> Result<(), PortError> {
        self.validate_for_claim(request)?;
        self.validate_at(now)
    }

    /// Returns whether this lease is expired at the supplied logical time.
    #[must_use]
    pub const fn is_expired_at(&self, now: LogicalTimeV1) -> bool {
        now.0 > self.lease_expires_at.0
    }

    /// Validates the lease and rejects acknowledgement after expiry.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::TimedOut`] when the lease has expired, or
    /// [`PortError::Invariant`] when its record is invalid.
    pub fn validate_at(&self, now: LogicalTimeV1) -> Result<(), PortError> {
        self.validate()?;
        if self.is_expired_at(now) {
            Err(PortError::TimedOut)
        } else {
            Ok(())
        }
    }

    /// Produces an acknowledged record only while this lease is valid.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::TimedOut`] after expiry or
    /// [`PortError::Invariant`] for an invalid record.
    pub fn acknowledge_at(&self, now: LogicalTimeV1) -> Result<OutboxRecordV1, PortError> {
        self.validate_at(now)?;
        self.record.clone().acknowledge()
    }

    /// Produces a renewed lease only before expiry and only with a later expiry.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::TimedOut`] after expiry or
    /// [`PortError::Invariant`] for a non-extending expiry.
    pub fn renew_at(
        &self,
        now: LogicalTimeV1,
        new_expiry: LogicalTimeV1,
    ) -> Result<Self, PortError> {
        self.validate_at(now)?;
        if self.record.acknowledgement != OutboxAcknowledgementV1::Pending
            || new_expiry.0 <= self.lease_expires_at.0
        {
            return Err(PortError::Invariant);
        }
        let mut renewed = self.clone();
        renewed.lease_expires_at = new_expiry;
        Ok(renewed)
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
    /// The request's inclusive logical deadline has passed.
    #[error("external effect request deadline has passed")]
    DeadlineExceeded,
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

/// Validation failure while constructing an ordered outcome-log view.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeLogValidationError {
    /// The outcome does not belong to the accumulator's pinned scope.
    #[error("outcome log scope does not match")]
    ScopeMismatch,
    /// The outcome sequence is not the next contiguous sequence.
    #[error("outcome log sequence is not contiguous")]
    NonContiguousSequence,
    /// The immutable outcome identity was already observed in this log.
    #[error("outcome log contains a duplicate outcome identity")]
    DuplicateOutcomeId,
    /// The bounded in-memory log limit would be exceeded.
    #[error("outcome log exceeds the replay bound")]
    LimitExceeded,
    /// The DTO's versioned schema or internal validation failed.
    #[error("outcome log contains an invalid outcome")]
    InvalidOutcome,
    /// Advancing the sequence would overflow.
    #[error("outcome log sequence overflowed")]
    SequenceOverflow,
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

/// Typed failure for reconciliation consistency-window handling.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ConsistencyWindowValidationError {
    /// The adapter supplied a window whose end precedes its observation time.
    #[error("reconciliation consistency window is invalid")]
    InvalidWindow,
    /// Retry was requested before authoritative absence could be established.
    #[error("reconciliation consistency window has not elapsed")]
    WindowNotElapsed,
    /// An unknown or committed effect cannot be treated as retry-safe.
    #[error("reconciliation result is not retry-safe")]
    NotRetrySafe,
}

/// Reconciliation result paired with the adapter's authoritative observation
/// time and consistency horizon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalReconciliationWindowV1 {
    /// Typed reconciliation result.
    pub result: CanonicalReconciliationV1,
    /// Logical time at which the authoritative source was checked.
    pub checked_at: LogicalTimeV1,
    /// Earliest logical time at which `NotCommitted` may authorize retry.
    pub consistency_until: LogicalTimeV1,
}

/// Safe next step derived from authoritative reconciliation evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryDispositionV1 {
    /// The effect is committed and must not be retried.
    Committed {
        /// Verified canonical event proving commitment.
        event: CanonicalEventDtoV1,
    },
    /// Authoritative absence is not yet safe to use before the horizon.
    Wait {
        /// Earliest logical time at which retry may be reconsidered.
        until: LogicalTimeV1,
    },
    /// The action is safe to retry after authoritative absence and horizon.
    Retry {
        /// Exact action identity eligible for retry.
        action_id: ActionId,
    },
    /// Evidence is ambiguous and requires escalation/reconciliation.
    Escalate {
        /// Exact action identity requiring operator or later evidence.
        action_id: ActionId,
    },
}

impl CanonicalReconciliationWindowV1 {
    /// Creates and validates a consistency-window result.
    ///
    /// # Errors
    ///
    /// Returns [`ConsistencyWindowValidationError::InvalidWindow`] when the
    /// horizon precedes the observation time.
    pub fn new(
        result: CanonicalReconciliationV1,
        checked_at: LogicalTimeV1,
        consistency_until: LogicalTimeV1,
    ) -> Result<Self, ConsistencyWindowValidationError> {
        if consistency_until.0 < checked_at.0 {
            return Err(ConsistencyWindowValidationError::InvalidWindow);
        }
        Ok(Self {
            result,
            checked_at,
            consistency_until,
        })
    }

    /// Validates whether this result authorizes a retry at `now`.
    ///
    /// # Errors
    ///
    /// Returns a typed error until the horizon elapses, or for committed/
    /// unknown results that are never retry permission.
    pub fn require_retry_safe_at(
        &self,
        now: LogicalTimeV1,
    ) -> Result<ActionId, ConsistencyWindowValidationError> {
        if now.0 < self.consistency_until.0 {
            return Err(ConsistencyWindowValidationError::WindowNotElapsed);
        }
        match &self.result {
            CanonicalReconciliationV1::NotCommitted { action_id, .. } => Ok(action_id.clone()),
            CanonicalReconciliationV1::Committed { .. }
            | CanonicalReconciliationV1::Unknown { .. } => {
                Err(ConsistencyWindowValidationError::NotRetrySafe)
            }
        }
    }

    /// Validates the wrapped evidence against the exact action being recovered.
    ///
    /// # Errors
    ///
    /// Returns a typed reconciliation error when the result is cross-scoped,
    /// action-substituted, or contains malformed committed evidence.
    pub fn validate_for_action(
        &self,
        action: &ProcessActionDtoV1,
    ) -> Result<(), ReconciliationValidationError> {
        self.result.validate_for_action(action)
    }

    /// Derives a fail-closed recovery disposition for an exact action.
    ///
    /// # Errors
    ///
    /// Returns a typed reconciliation error when the evidence is not bound to
    /// the requested action.
    pub fn disposition_at(
        &self,
        action: &ProcessActionDtoV1,
        now: LogicalTimeV1,
    ) -> Result<RecoveryDispositionV1, ReconciliationValidationError> {
        self.validate_for_action(action)?;
        Ok(match &self.result {
            CanonicalReconciliationV1::Committed { event, .. } => {
                RecoveryDispositionV1::Committed {
                    event: event.clone(),
                }
            }
            CanonicalReconciliationV1::NotCommitted { .. } if now.0 < self.consistency_until.0 => {
                RecoveryDispositionV1::Wait {
                    until: self.consistency_until,
                }
            }
            CanonicalReconciliationV1::NotCommitted { action_id, .. } => {
                RecoveryDispositionV1::Retry {
                    action_id: action_id.clone(),
                }
            }
            CanonicalReconciliationV1::Unknown { action_id, .. } => {
                RecoveryDispositionV1::Escalate {
                    action_id: action_id.clone(),
                }
            }
        })
    }
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
    /// Submit an explicit terminal override after review and evidence.
    TerminalOverride,
}

/// Minimum control required before a mutable process operation can proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationRequirementV1 {
    /// One authenticated principal may authorize the operation.
    AuthenticatedPrincipal,
    /// Two distinct authenticated principals are required.
    DistinctPrincipals,
}

impl ProcessAuthorizationOperationV1 {
    /// Returns the minimum control level for this high-value operation.
    #[must_use]
    pub const fn minimum_requirement(self) -> AuthorizationRequirementV1 {
        match self {
            Self::Start | Self::Cancel | Self::Retry => {
                AuthorizationRequirementV1::AuthenticatedPrincipal
            }
            Self::DecideReview | Self::TerminalOverride => {
                AuthorizationRequirementV1::DistinctPrincipals
            }
        }
    }
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

    /// Returns whether the request is past its inclusive logical deadline.
    #[must_use]
    pub const fn is_expired_at(&self, now: LogicalTimeV1) -> bool {
        match self.deadline {
            Some(deadline) => now.0 > deadline.0,
            None => false,
        }
    }

    /// Validates the request and rejects execution after its deadline.
    ///
    /// # Errors
    ///
    /// Returns [`EffectReconciliationValidationError::DeadlineExceeded`] when
    /// the logical deadline has passed.
    pub fn validate_at(
        &self,
        now: LogicalTimeV1,
    ) -> Result<(), EffectReconciliationValidationError> {
        self.validate()?;
        if self.is_expired_at(now) {
            Err(EffectReconciliationValidationError::DeadlineExceeded)
        } else {
            Ok(())
        }
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

/// Bounded request for atomically claiming due timers for one process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerClaimRequestV1 {
    /// Process scope whose timers may be claimed.
    pub scope: ProcessScopeV1,
    /// Authenticated worker receiving the fencing leases.
    pub owner: PrincipalId,
    /// Logical time used to determine due timers.
    pub now: LogicalTimeV1,
    /// Maximum timers to claim.
    pub limit: NonZeroU16,
}

impl TimerClaimRequestV1 {
    /// Creates a bounded timer-claim request.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::QuotaExceeded`] when the batch is too large.
    pub fn new(
        scope: ProcessScopeV1,
        owner: PrincipalId,
        now: LogicalTimeV1,
        limit: NonZeroU16,
    ) -> Result<Self, PortError> {
        let request = Self {
            scope,
            owner,
            now,
            limit,
        };
        request.validate()?;
        Ok(request)
    }

    /// Validates the bounded claim size.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::QuotaExceeded`] when the batch is too large.
    pub const fn validate(&self) -> Result<(), PortError> {
        if self.limit.get() > MAX_TIMER_CLAIM_BATCH {
            Err(PortError::QuotaExceeded)
        } else {
            Ok(())
        }
    }
}

/// Fenced lease for one due timer delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerLeaseV1 {
    /// Full timer schedule retained under the lease.
    pub timer: TimerScheduleV1,
    /// Worker that owns this lease.
    pub owner: PrincipalId,
    /// Opaque fencing token preventing stale acknowledgements.
    pub token: OutboxLeaseTokenV1,
    /// Inclusive logical expiry of the claim.
    pub lease_expires_at: LogicalTimeV1,
}

impl TimerLeaseV1 {
    /// Validates timer shape, process scope, owner and due-time claim.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invariant`] for substitution or malformed timers.
    pub fn validate_for_claim(&self, request: &TimerClaimRequestV1) -> Result<(), PortError> {
        request.validate()?;
        self.timer
            .validate()
            .map_err(|_timer_error| PortError::Invariant)?;
        if self.timer.action.scope() != request.scope
            || self.owner != request.owner
            || self.timer.due_at > request.now
        {
            return Err(PortError::Invariant);
        }
        if request.now > self.lease_expires_at {
            return Err(PortError::TimedOut);
        }
        Ok(())
    }

    /// Validates lease expiry at a logical boundary.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::TimedOut`] after expiry.
    pub fn validate_at(&self, now: LogicalTimeV1) -> Result<(), PortError> {
        self.timer
            .validate()
            .map_err(|_timer_error| PortError::Invariant)?;
        if now > self.lease_expires_at {
            Err(PortError::TimedOut)
        } else {
            Ok(())
        }
    }

    /// Produces the acknowledged timer schedule only while the lease is valid.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::TimedOut`] after expiry or
    /// [`PortError::Invariant`] for a malformed timer lease.
    pub fn acknowledge_at(&self, now: LogicalTimeV1) -> Result<TimerScheduleV1, PortError> {
        self.validate_at(now)?;
        Ok(self.timer.clone())
    }
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

/// Pure bounded accumulator for an ordered immutable outcome log.
///
/// This type is deliberately not a store: it gives adapters and recovery
/// code one shared validator for append order, scope pinning and idempotent
/// outcome identities. Persistence and compare-and-append remain the adapter's
/// responsibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutcomeLogV1 {
    /// Immutable process and definition scope of every outcome.
    scope: ProcessScopeV1,
    /// Ordered outcomes accepted so far.
    outcomes: Vec<ProcessOutcomeDtoV1>,
    #[serde(skip)]
    seen_outcome_ids: HashSet<OutcomeId>,
}

impl<'de> Deserialize<'de> for OutcomeLogV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            scope: ProcessScopeV1,
            outcomes: Vec<ProcessOutcomeDtoV1>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::from_ordered(wire.scope, &wire.outcomes).map_err(D::Error::custom)
    }
}

impl OutcomeLogV1 {
    /// Creates an empty log whose first accepted outcome must use sequence zero.
    #[must_use]
    pub fn new(scope: ProcessScopeV1) -> Self {
        Self {
            scope,
            outcomes: Vec::new(),
            seen_outcome_ids: HashSet::new(),
        }
    }

    /// Returns the immutable scope pinned to this log.
    #[must_use]
    pub const fn scope(&self) -> &ProcessScopeV1 {
        &self.scope
    }

    /// Returns accepted outcomes in contiguous sequence order.
    #[must_use]
    pub fn outcomes(&self) -> &[ProcessOutcomeDtoV1] {
        &self.outcomes
    }

    /// Returns the number of accepted outcomes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.outcomes.len()
    }

    /// Returns whether no outcomes have been accepted.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    /// Rebuilds a bounded log from an already ordered durable slice.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the slice is malformed, out of order,
    /// cross-scoped, duplicated, or exceeds the replay bound.
    pub fn from_ordered(
        scope: ProcessScopeV1,
        outcomes: &[ProcessOutcomeDtoV1],
    ) -> Result<Self, OutcomeLogValidationError> {
        let mut log = Self::new(scope);
        log.append(outcomes)?;
        Ok(log)
    }

    /// Appends outcomes only when all invariants hold.
    ///
    /// Validation is performed before mutating the accumulator, so a failed
    /// batch cannot leave a partially applied in-memory projection.
    ///
    /// # Errors
    ///
    /// Returns a typed error when any outcome is malformed, cross-scoped,
    /// duplicated, out of order, or would exceed the replay bound.
    pub fn append(
        &mut self,
        outcomes: &[ProcessOutcomeDtoV1],
    ) -> Result<(), OutcomeLogValidationError> {
        let new_len = self
            .outcomes
            .len()
            .checked_add(outcomes.len())
            .ok_or(OutcomeLogValidationError::LimitExceeded)?;
        if new_len > MAX_OUTCOMES_PER_REPLAY_LOG {
            return Err(OutcomeLogValidationError::LimitExceeded);
        }
        let mut expected = u64::try_from(self.outcomes.len())
            .map_err(|_conversion_error| OutcomeLogValidationError::SequenceOverflow)?;
        let mut new_ids = HashSet::with_capacity(outcomes.len());
        for outcome in outcomes {
            if outcome.validate().is_err() {
                return Err(OutcomeLogValidationError::InvalidOutcome);
            }
            if outcome.scope() != self.scope {
                return Err(OutcomeLogValidationError::ScopeMismatch);
            }
            if outcome.sequence != expected {
                return Err(OutcomeLogValidationError::NonContiguousSequence);
            }
            if self.seen_outcome_ids.contains(&outcome.outcome_id)
                || !new_ids.insert(outcome.outcome_id.clone())
            {
                return Err(OutcomeLogValidationError::DuplicateOutcomeId);
            }
            expected = expected
                .checked_add(1)
                .ok_or(OutcomeLogValidationError::SequenceOverflow)?;
        }
        self.outcomes.extend_from_slice(outcomes);
        self.seen_outcome_ids.extend(new_ids);
        Ok(())
    }

    /// Returns the next sequence required by [`Self::append`].
    #[must_use]
    pub fn next_sequence(&self) -> u64 {
        u64::try_from(self.outcomes.len()).unwrap_or(u64::MAX)
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

/// Typed validation failure for an atomic commit acknowledgement.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AtomicProcessCommitReceiptValidationError {
    /// The submitted commit itself failed validation.
    #[error("atomic commit receipt cannot validate an invalid commit")]
    InvalidCommit,
    /// The acknowledgement does not identify the submitted commit's final sequence.
    #[error("atomic commit receipt sequence does not match the submitted commit")]
    SequenceMismatch,
    /// A duplicate-input flag cannot be asserted when no input was submitted.
    #[error("atomic commit receipt claims a duplicate input without an input")]
    DuplicateInputWithoutInput,
}

impl AtomicProcessCommitReceiptV1 {
    /// Validates the acknowledgement against the exact atomic commit request.
    ///
    /// # Errors
    ///
    /// Returns a typed error when sequence arithmetic or duplicate-input
    /// semantics do not match the request.
    pub fn validate_for(
        &self,
        commit: &AtomicProcessCommitV1,
    ) -> Result<(), AtomicProcessCommitReceiptValidationError> {
        if commit.validate().is_err() {
            return Err(AtomicProcessCommitReceiptValidationError::InvalidCommit);
        }
        let expected_last = commit
            .expected_sequence
            .checked_add((commit.outcomes.len().saturating_sub(1)) as u64);
        if expected_last != Some(self.committed_through_sequence) {
            return Err(AtomicProcessCommitReceiptValidationError::SequenceMismatch);
        }
        if self.duplicate_input && commit.input.is_none() {
            return Err(AtomicProcessCommitReceiptValidationError::DuplicateInputWithoutInput);
        }
        Ok(())
    }
}

/// Durable append-only process outcome store.
#[async_trait]
pub trait DefinitionRegistry: Send + Sync {
    /// Registers one immutable definition after schema and bound validation.
    /// Registration must be idempotent for the same identity and digest and
    /// must reject a changed digest under an existing identity/version.
    async fn register(
        &self,
        definition: &ProcessDefinitionDtoV1,
    ) -> Result<DefinitionRegistrationReceiptV1, PortError>;

    /// Looks up the exact immutable definition requested by a process.
    async fn get(&self, lookup: &DefinitionLookupV1) -> Result<ProcessDefinitionDtoV1, PortError>;

    /// Registers an explicit version migration after validating its binding.
    async fn register_migration(
        &self,
        migration: &DefinitionMigrationV1,
    ) -> Result<DefinitionMigrationReceiptV1, PortError>;
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

    /// Acknowledges one exact, unexpired action delivery after successful dispatch.
    ///
    /// Implementations must call [`OutboxLeaseV1::validate_at`] with `now`
    /// before changing durable acknowledgement state.
    async fn acknowledge(&self, lease: &OutboxLeaseV1, now: LogicalTimeV1)
    -> Result<(), PortError>;

    /// Renews one exact unexpired lease with a later logical expiry.
    async fn renew(
        &self,
        lease: &OutboxLeaseV1,
        now: LogicalTimeV1,
        new_expiry: LogicalTimeV1,
    ) -> Result<OutboxLeaseV1, PortError>;
}

/// Durable inbox that deduplicates immutable source inputs.
#[async_trait]
pub trait Inbox: Send + Sync {
    /// Records an input exactly once before it is processed.
    /// The returned receipt lets callers treat a duplicate as a successful
    /// no-op without re-running the process decision.
    async fn accept(
        &self,
        input: &ProcessInputDtoV1,
    ) -> Result<InboxAcceptanceReceiptV1, PortError>;
}

/// Durable process-action dispatch boundary.
#[async_trait]
pub trait ActionDispatcher: Send + Sync {
    /// Dispatches one action using its stable action identity as idempotency key.
    async fn dispatch(
        &self,
        action: &ProcessActionDtoV1,
    ) -> Result<ActionDispatchReceiptV1, PortError>;
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

/// Durable due-timer claim boundary with fencing semantics.
#[async_trait]
pub trait TimerClaimStore: Send + Sync {
    /// Atomically claims due timers for one owner and returns fenced leases.
    async fn claim_due(
        &self,
        request: &TimerClaimRequestV1,
    ) -> Result<Vec<TimerLeaseV1>, PortError>;

    /// Acknowledges one exact, unexpired timer lease after inbox delivery.
    async fn acknowledge(&self, lease: &TimerLeaseV1, now: LogicalTimeV1) -> Result<(), PortError>;
}

/// Injected source of fresh opaque fencing tokens for worker leases.
#[async_trait]
pub trait LeaseTokenSource: Send + Sync {
    /// Allocates a non-zero token for the supplied pinned process scope.
    /// Implementations must guarantee uniqueness among concurrently live
    /// leases and must never recycle a token while an old lease may exist.
    async fn next_lease_token(
        &self,
        scope: &ProcessScopeV1,
    ) -> Result<OutboxLeaseTokenV1, PortError>;
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

/// Redacted, typed observability boundary.
///
/// Implementations receive only bounded diagnostics; raw error messages and
/// payloads are intentionally absent from this port.
#[async_trait]
pub trait DiagnosticSink: Send + Sync {
    /// Records one diagnostic for its pinned process scope.
    async fn record(&self, diagnostic: &RedactedDiagnosticV1) -> Result<(), PortError>;
}

/// Canonical-state boundary implemented by a StateChronicle adapter.
#[async_trait]
pub trait CanonicalState: Send + Sync {
    /// Submits a command with the Penelope action ID as canonical idempotency ID.
    async fn submit(
        &self,
        command: &CanonicalCommandDtoV1,
    ) -> Result<CanonicalSubmitReceiptV1, PortError>;
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
    async fn open(&self, review: &ManualReviewDtoV1) -> Result<ManualReviewReceiptV1, PortError>;
    /// Claims a review case without changing the process projection directly.
    async fn claim(&self, claim: &ManualReviewClaimV1) -> Result<ManualReviewReceiptV1, PortError>;
    /// Records an authorized immutable resolution for subsequent inbox delivery.
    async fn decide(
        &self,
        decision: &ManualReviewDecisionV1,
    ) -> Result<ManualReviewReceiptV1, PortError>;
}

fn canonical_port_bytes<T: Serialize>(
    value: &T,
) -> Result<Vec<u8>, penelope_domain::CanonicalEncodingError> {
    serde_json::to_vec(value)
        .map_err(|_serialization_error| penelope_domain::CanonicalEncodingError::Serialization)
}

macro_rules! canonical_port_impl {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl CanonicalWireBytesV1 for $ty {
                fn canonical_wire_bytes(&self) -> Result<Vec<u8>, penelope_domain::CanonicalEncodingError> {
                    canonical_port_bytes(self)
                }
            }
        )+
    };
}

canonical_port_impl!(
    DefinitionLookupV1,
    DefinitionRegistrationReceiptV1,
    DefinitionMigrationReceiptV1,
    InboxAcceptanceReceiptV1,
    ActionDispatchReceiptV1,
    CanonicalSubmitReceiptV1,
    ManualReviewReceiptV1,
    ManualReviewOperationV1,
    OutboxLeaseTokenV1,
    OutboxAcknowledgementV1,
    OutboxRecordV1,
    OutboxClaimRequestV1,
    OutboxLeaseV1,
    DiagnosticClassV1,
    RedactedDiagnosticV1,
    QuotaKindV1,
    QuotaRequestV1,
    CanonicalReconciliationV1,
    CanonicalReconciliationWindowV1,
    RecoveryDispositionV1,
    ManualReviewResolutionV1,
    ManualReviewControlV1,
    ProcessAuthorizationOperationV1,
    AuthorizationRequirementV1,
    ProcessAuthorizationRequestV1,
    ProcessAuthorizationDecisionV1,
    ExternalEffectStateV1,
    EffectDispatchRequestV1,
    ExternalEffectEvidenceV1,
    TimerScheduleV1,
    TimerClaimRequestV1,
    TimerLeaseV1,
    ManualReviewClaimV1,
    ManualReviewDecisionV1,
    AtomicProcessCommitV1,
    OutcomeReplayRequestV1,
    OutcomeReplayPageV1,
    OutcomeLogV1,
    AtomicProcessCommitReceiptV1,
);

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
        let request = OutboxClaimRequestV1::new(
            outcome(0, id("out_claim_scope")).scope(),
            id("pri_worker"),
            NonZeroU16::MIN,
        )
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
        let owner_mismatch = OutboxClaimRequestV1::new(
            outcome(0, id("out_claim_scope")).scope(),
            id("pri_other"),
            NonZeroU16::MIN,
        )
        .unwrap();
        assert_eq!(
            lease.validate_for_claim(&owner_mismatch),
            Err(PortError::Invariant)
        );
    }

    #[test]
    fn outbox_lease_expiry_is_checked_at_logical_boundary() {
        let action = action();
        let request = OutboxClaimRequestV1::new(
            ProcessScopeV1::new(
                action.tenant_id.clone(),
                action.process_id.clone(),
                action.definition_id.clone(),
                action.definition_version.clone(),
                action.definition_digest,
            ),
            id("pri_worker"),
            NonZeroU16::MIN,
        )
        .unwrap();
        let lease = OutboxLeaseV1 {
            record: OutboxRecordV1::new(action),
            owner: id("pri_worker"),
            token: OutboxLeaseTokenV1::new(NonZeroU64::MIN),
            lease_expires_at: LogicalTimeV1(5),
        };
        assert!(!lease.is_expired_at(LogicalTimeV1(5)));
        assert!(lease.is_expired_at(LogicalTimeV1(6)));
        assert_eq!(
            lease.validate_at(LogicalTimeV1(6)),
            Err(PortError::TimedOut)
        );
        assert_eq!(
            lease.acknowledge_at(LogicalTimeV1(6)),
            Err(PortError::TimedOut)
        );
        assert_eq!(
            lease
                .acknowledge_at(LogicalTimeV1(5))
                .unwrap()
                .acknowledgement,
            OutboxAcknowledgementV1::Acknowledged
        );
        assert_eq!(
            lease
                .renew_at(LogicalTimeV1(4), LogicalTimeV1(6))
                .unwrap()
                .lease_expires_at,
            LogicalTimeV1(6)
        );
        assert_eq!(
            lease.renew_at(LogicalTimeV1(4), LogicalTimeV1(5)),
            Err(PortError::Invariant)
        );
        assert!(
            lease
                .validate_for_claim_at(&request, LogicalTimeV1(5))
                .is_ok()
        );
        let acknowledged = lease.acknowledge_at(LogicalTimeV1(5)).unwrap();
        let mut acknowledged_lease = lease;
        acknowledged_lease.record = acknowledged;
        assert_eq!(
            acknowledged_lease.renew_at(LogicalTimeV1(4), LogicalTimeV1(6)),
            Err(PortError::Invariant)
        );
    }

    #[test]
    fn outbox_claim_request_rejects_an_oversized_batch() {
        let request = OutboxClaimRequestV1 {
            scope: outcome(0, id("out_claim_scope")).scope(),
            owner: id("pri_worker"),
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
    fn external_effect_deadline_is_inclusive_and_fail_closed_after_expiry() {
        let request = EffectDispatchRequestV1::new(action()).with_deadline(LogicalTimeV1(5));
        assert!(!request.is_expired_at(LogicalTimeV1(5)));
        assert!(request.validate_at(LogicalTimeV1(6)).is_err());
        assert_eq!(
            request.validate_at(LogicalTimeV1(6)),
            Err(EffectReconciliationValidationError::DeadlineExceeded)
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
    fn reconciliation_window_only_allows_retry_after_horizon() {
        let result = CanonicalReconciliationV1::NotCommitted {
            scope: action().scope(),
            action_id: action().action_id,
        };
        let window =
            CanonicalReconciliationWindowV1::new(result, LogicalTimeV1(10), LogicalTimeV1(20))
                .unwrap();
        assert_eq!(
            window.require_retry_safe_at(LogicalTimeV1(19)),
            Err(ConsistencyWindowValidationError::WindowNotElapsed)
        );
        assert_eq!(
            window.require_retry_safe_at(LogicalTimeV1(20)),
            Ok(id("act_dispatch"))
        );
        assert_eq!(window.validate_for_action(&action()), Ok(()));
        assert_eq!(
            window.disposition_at(&action(), LogicalTimeV1(19)),
            Ok(RecoveryDispositionV1::Wait {
                until: LogicalTimeV1(20)
            })
        );
        assert_eq!(
            window.disposition_at(&action(), LogicalTimeV1(20)),
            Ok(RecoveryDispositionV1::Retry {
                action_id: id("act_dispatch")
            })
        );
        let mut wrong_action = action();
        wrong_action.action_id = id("act_other");
        assert_eq!(
            window.validate_for_action(&wrong_action),
            Err(ReconciliationValidationError::ActionMismatch)
        );
        assert_eq!(
            CanonicalReconciliationWindowV1::new(
                window.result,
                LogicalTimeV1(21),
                LogicalTimeV1(20),
            ),
            Err(ConsistencyWindowValidationError::InvalidWindow)
        );
        let committed = CanonicalReconciliationWindowV1::new(
            CanonicalReconciliationV1::Unknown {
                scope: action().scope(),
                action_id: id("act_dispatch"),
            },
            LogicalTimeV1(0),
            LogicalTimeV1(0),
        )
        .unwrap();
        assert_eq!(
            committed.require_retry_safe_at(LogicalTimeV1(0)),
            Err(ConsistencyWindowValidationError::NotRetrySafe)
        );
        assert_eq!(
            committed.disposition_at(&action(), LogicalTimeV1(0)),
            Ok(RecoveryDispositionV1::Escalate {
                action_id: id("act_dispatch")
            })
        );
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

    #[test]
    fn outcome_log_append_is_atomic_and_rejects_reorder_scope_and_duplicates() {
        let scope = outcome(0, id("out_first")).scope();
        let first = outcome(0, id("out_first"));
        let second = outcome(1, id("out_second"));
        let mut log = OutcomeLogV1::new(scope);
        assert_eq!(log.append(&[first, second]), Ok(()));
        assert_eq!(log.next_sequence(), 2);

        let before_failed_append = log.clone();
        let reordered = outcome(4, id("out_reordered"));
        assert_eq!(
            log.append(&[reordered]),
            Err(OutcomeLogValidationError::NonContiguousSequence)
        );
        assert_eq!(log, before_failed_append);

        let duplicate = outcome(2, id("out_second"));
        assert_eq!(
            log.append(&[duplicate]),
            Err(OutcomeLogValidationError::DuplicateOutcomeId)
        );
        assert_eq!(log, before_failed_append);

        let other_scope = ProcessScopeV1::new(
            id("tnt_other"),
            id("prc_trade"),
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([9; 32]),
        );
        let cross_scope = ProcessOutcomeDtoV1::new(
            other_scope,
            2,
            ProcessOutcomeFactV1::new(
                id("out_cross"),
                CausationIdV1::Action(id("act_cause")),
                OutcomeActorV1::System,
                LogicalTimeV1(1),
                ProcessOutcomeKindV1::ActionPlanned,
                ContentDigest([1; 32]),
            ),
        );
        assert_eq!(
            log.append(&[cross_scope]),
            Err(OutcomeLogValidationError::ScopeMismatch)
        );
        assert_eq!(log, before_failed_append);

        let encoded = serde_json::to_vec(&log).unwrap();
        let mut decoded: OutcomeLogV1 = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, log);
        assert_eq!(decoded.append(&[outcome(2, id("out_third"))]), Ok(()));
    }

    #[test]
    fn redacted_diagnostics_are_bounded_and_never_require_message_matching() {
        let scope = outcome(0, id("out_diag")).scope();
        let diagnostic = RedactedDiagnosticV1::new(
            scope,
            DiagnosticClassV1::from_port_error(PortError::Unauthorized),
            penelope_domain::ContentDigest([8; 32]),
            MAX_REDACTED_DIAGNOSTIC_BYTES,
        )
        .unwrap();
        assert_eq!(diagnostic.class, DiagnosticClassV1::Authorization);
        assert_eq!(
            DiagnosticClassV1::from_port_error(PortError::Ambiguous),
            DiagnosticClassV1::Ambiguous
        );
        assert_eq!(
            RedactedDiagnosticV1::new(
                diagnostic.scope,
                DiagnosticClassV1::Invariant,
                diagnostic.evidence_digest,
                MAX_REDACTED_DIAGNOSTIC_BYTES.saturating_add(1),
            ),
            Err(DiagnosticValidationError::MetadataLimitExceeded)
        );
    }

    #[test]
    fn quota_requests_are_typed_bounded_and_fail_closed() {
        let valid = QuotaRequestV1::new(
            QuotaKindV1::OutstandingActions,
            NonZeroU32::new(4).unwrap(),
            NonZeroU32::new(8).unwrap(),
        )
        .unwrap();
        assert_eq!(valid.validate(), Ok(()));
        assert!(!valid.canonical_wire_bytes().unwrap().is_empty());
        assert_eq!(
            QuotaRequestV1::new(
                QuotaKindV1::OutcomeWrites,
                NonZeroU32::new(9).unwrap(),
                NonZeroU32::new(8).unwrap(),
            ),
            Err(QuotaValidationError::RequestExceedsCapacity)
        );
        assert_eq!(
            QuotaRequestV1::new(
                QuotaKindV1::OpenProcesses,
                NonZeroU32::new(MAX_QUOTA_CAPACITY).unwrap(),
                NonZeroU32::new(MAX_QUOTA_CAPACITY.saturating_add(1)).unwrap(),
            ),
            Err(QuotaValidationError::CapacityLimitExceeded)
        );
    }

    #[test]
    fn authorization_matrix_requires_distinct_principals_for_high_value_actions() {
        assert_eq!(
            ProcessAuthorizationOperationV1::Start.minimum_requirement(),
            AuthorizationRequirementV1::AuthenticatedPrincipal
        );
        assert_eq!(
            ProcessAuthorizationOperationV1::Retry.minimum_requirement(),
            AuthorizationRequirementV1::AuthenticatedPrincipal
        );
        assert_eq!(
            ProcessAuthorizationOperationV1::DecideReview.minimum_requirement(),
            AuthorizationRequirementV1::DistinctPrincipals
        );
        assert_eq!(
            ProcessAuthorizationOperationV1::TerminalOverride.minimum_requirement(),
            AuthorizationRequirementV1::DistinctPrincipals
        );
    }

    #[test]
    fn inbox_receipt_is_bound_to_the_exact_submitted_input() {
        let submitted = input();
        let receipt = InboxAcceptanceReceiptV1::new(
            submitted.tenant_id.clone(),
            submitted.process_id.clone(),
            submitted.input_id.clone(),
            false,
        );
        assert_eq!(receipt.validate_for(&submitted), Ok(()));
        let wrong = ProcessInputDtoV1::new(
            submitted.tenant_id.clone(),
            submitted.process_id.clone(),
            id("inp_other"),
            submitted.kind,
            submitted.payload_digest,
        );
        assert_eq!(
            receipt.validate_for(&wrong),
            Err(InboxReceiptValidationError::InputMismatch)
        );
        let mut wrong_scope = wrong;
        wrong_scope.tenant_id = id("tnt_other");
        assert_eq!(
            receipt.validate_for(&wrong_scope),
            Err(InboxReceiptValidationError::ScopeMismatch)
        );
    }

    #[test]
    fn manual_review_receipt_is_bound_to_the_exact_case() {
        let review_id: ReviewId = id("rev_case");
        let receipt = ManualReviewReceiptV1::new_for_operation(
            review_id.clone(),
            ManualReviewOperationV1::Claim,
            true,
        );
        assert_eq!(receipt.validate_for(&review_id), Ok(()));
        assert_eq!(
            receipt.validate_for_operation(&review_id, ManualReviewOperationV1::Claim),
            Ok(())
        );
        assert_eq!(
            receipt.validate_for_operation(&review_id, ManualReviewOperationV1::Decide),
            Err(ManualReviewReceiptValidationError::OperationMismatch)
        );
        assert_eq!(
            receipt.validate_for(&id("rev_other")),
            Err(ManualReviewReceiptValidationError::ReviewMismatch)
        );
    }

    #[test]
    fn action_dispatch_receipt_is_bound_to_the_exact_action() {
        let requested = action();
        let receipt =
            ActionDispatchReceiptV1::new(requested.scope(), requested.action_id.clone(), true);
        assert_eq!(receipt.validate_for(&requested), Ok(()));
        let mut other = action();
        other.action_id = id("act_other");
        assert_eq!(
            receipt.validate_for(&other),
            Err(ActionReceiptValidationError::ActionMismatch)
        );
        let mut wrong_scope = action();
        wrong_scope.tenant_id = id("tnt_other");
        assert_eq!(
            receipt.validate_for(&wrong_scope),
            Err(ActionReceiptValidationError::ScopeMismatch)
        );
    }

    #[test]
    fn canonical_submit_receipt_is_bound_to_the_exact_command_action() {
        let submitted = CanonicalCommandDtoV1 {
            schema: penelope_domain::SchemaV1::CanonicalCommand,
            tenant_id: id("tnt_game"),
            action_id: id("act_dispatch"),
            operation: id("op_trade"),
            resource_ids: vec![id("res_wallet")],
            payload_digest: ContentDigest([4; 32]),
        };
        let receipt = CanonicalSubmitReceiptV1::new(
            submitted.tenant_id.clone(),
            submitted.action_id.clone(),
            true,
        );
        assert_eq!(receipt.validate_for(&submitted), Ok(()));
        let mut other = submitted.clone();
        other.action_id = id("act_other");
        assert_eq!(
            receipt.validate_for(&other),
            Err(CanonicalSubmitReceiptValidationError::ActionMismatch)
        );
        let mut wrong_tenant = submitted;
        wrong_tenant.tenant_id = id("tnt_other");
        assert_eq!(
            receipt.validate_for(&wrong_tenant),
            Err(CanonicalSubmitReceiptValidationError::ScopeMismatch)
        );
    }

    #[test]
    fn definition_registration_receipt_is_bound_to_identity_version_and_digest() {
        let definition = ProcessDefinitionDtoV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([9; 32]),
            vec![id("stp_dispatch")],
        )
        .unwrap();
        let receipt = DefinitionRegistrationReceiptV1::new(
            definition.definition_id.clone(),
            definition.definition_version.clone(),
            definition.definition_digest,
            false,
        );
        assert_eq!(receipt.validate_for(&definition), Ok(()));
        let wrong = DefinitionRegistrationReceiptV1::new(
            definition.definition_id.clone(),
            definition.definition_version.clone(),
            ContentDigest([8; 32]),
            false,
        );
        assert_eq!(
            wrong.validate_for(&definition),
            Err(DefinitionRegistrationReceiptValidationError::DefinitionMismatch)
        );
    }

    #[test]
    fn migration_registration_receipt_is_bound_to_migration_identity() {
        let migration = DefinitionMigrationV1::new(
            id("mig_trade"),
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([9; 32]),
            id("dfv_two"),
            ContentDigest([8; 32]),
        )
        .unwrap();
        let receipt = DefinitionMigrationReceiptV1::new(migration.migration_id.clone(), true);
        assert_eq!(receipt.validate_for(&migration), Ok(()));
        let wrong = DefinitionMigrationReceiptV1::new(id("mig_other"), false);
        assert_eq!(
            wrong.validate_for(&migration),
            Err(DefinitionMigrationReceiptValidationError::MigrationMismatch)
        );
    }

    #[test]
    fn atomic_commit_receipt_is_bound_to_sequence_and_input_semantics() {
        let mut accepted_outcome = outcome(4, id("out_commit"));
        accepted_outcome.causation_id = penelope_domain::CausationIdV1::Input(id("inp_event"));
        let commit =
            AtomicProcessCommitV1::new(4, Some(input()), vec![accepted_outcome], vec![action()])
                .unwrap();
        let receipt = AtomicProcessCommitReceiptV1 {
            committed_through_sequence: 4,
            duplicate_input: true,
        };
        assert_eq!(receipt.validate_for(&commit), Ok(()));
        let wrong_sequence = AtomicProcessCommitReceiptV1 {
            committed_through_sequence: 5,
            duplicate_input: false,
        };
        assert_eq!(
            wrong_sequence.validate_for(&commit),
            Err(AtomicProcessCommitReceiptValidationError::SequenceMismatch)
        );
        let no_input = AtomicProcessCommitV1::new(
            4,
            None,
            vec![outcome(4, id("out_commit_no_input"))],
            vec![action()],
        )
        .unwrap();
        let invalid_duplicate = AtomicProcessCommitReceiptV1 {
            committed_through_sequence: 4,
            duplicate_input: true,
        };
        assert_eq!(
            invalid_duplicate.validate_for(&no_input),
            Err(AtomicProcessCommitReceiptValidationError::DuplicateInputWithoutInput)
        );
        let malformed = AtomicProcessCommitV1 {
            expected_sequence: 4,
            input: None,
            canonical_source_event_id: None,
            outcomes: Vec::new(),
            actions: Vec::new(),
        };
        assert_eq!(
            receipt.validate_for(&malformed),
            Err(AtomicProcessCommitReceiptValidationError::InvalidCommit)
        );
    }

    #[test]
    fn timer_claim_lease_requires_due_time_scope_owner_and_expiry() {
        let mut timer_action = action();
        timer_action.kind = penelope_domain::ProcessActionKindV1::Timer;
        let timer = TimerScheduleV1 {
            action: timer_action,
            due_at: LogicalTimeV1(5),
        };
        let request = TimerClaimRequestV1::new(
            timer.action.scope(),
            id("pri_worker"),
            LogicalTimeV1(5),
            NonZeroU16::MIN,
        )
        .unwrap();
        let lease = TimerLeaseV1 {
            timer,
            owner: id("pri_worker"),
            token: OutboxLeaseTokenV1::new(NonZeroU64::MIN),
            lease_expires_at: LogicalTimeV1(6),
        };
        assert_eq!(lease.validate_for_claim(&request), Ok(()));
        assert_eq!(lease.validate_at(LogicalTimeV1(6)), Ok(()));
        assert_eq!(
            lease.acknowledge_at(LogicalTimeV1(6)),
            Ok(lease.timer.clone())
        );
        assert_eq!(
            lease.validate_at(LogicalTimeV1(7)),
            Err(PortError::TimedOut)
        );
        assert_eq!(
            lease.acknowledge_at(LogicalTimeV1(7)),
            Err(PortError::TimedOut)
        );
    }
}
