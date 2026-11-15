//! Deterministic saga coordination, recoverable execution and optional timelines.
//!
//! Start with [`saga`] for linear or graph coordination and [`task`] for bounded
//! recovery and supervision. [`storage`] supplies protocol-specific durable ports;
//! [`integration`] supplies external canonical-evidence boundaries. Enable the
//! `timeline` feature for bounded rewindable computation.
//!
//! Decisions plan state changes and effects. Applications durably admit plans
//! before dispatch and supply verified evidence for external outcomes. No
//! database, broker, scheduler or external client is included in this facade.
//!
//! Existing root imports and internal-crate aliases remain available.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

/// Pure protocol primitives.
pub use penelope_core as core;
/// Versioned data-transfer objects.
pub use penelope_domain as domain;
/// Frequently used typed protocol values.
pub use penelope_domain::{
    ActionId, CanonicalCommand, CanonicalCommitId, CanonicalEncodingError, CanonicalEvent,
    CanonicalEventId, CanonicalWireBytes, CausationId, ContentDigest, DefinitionCompatibility,
    DefinitionCompatibilityError, DefinitionId, DefinitionMigration, DefinitionMigrationError,
    DefinitionVersion, DomainError, EffectKey, ExternalReferenceId, InputId, LogicalTime,
    ManualReview, MigrationId, OperationId, OutcomeActor, OutcomeId, ProcessAction,
    ProcessActionKind, ProcessDefinition, ProcessId, ProcessInput, ProcessInputEnvelope,
    ProcessInputKind, ProcessOutcome, ProcessOutcomeFact, ProcessOutcomeKind, ProcessScope,
    ResourceId, ReviewId, StepId, TenantId,
};
/// Application execution boundary.
pub use penelope_executor as executor;
/// Linear and graph saga coordination.
pub mod saga;
/// Inbound parsing and validation boundary.
pub use penelope_intent as intent;
/// User-facing typed process-input parser.
pub use penelope_intent::{IntentError, parse_process_input};
/// Backend-neutral ports implemented by the composition root.
pub use penelope_ports as ports;
/// Frequently used typed process-control port contracts.
pub use penelope_ports::{
    ActionDispatchReceipt, ActionReceiptValidationError, AtomicProcessCommit,
    AtomicProcessCommitReceipt, AtomicProcessCommitReceiptValidationError,
    AuthorizationRequirement, CanonicalReconciliation, CanonicalReconciliationWindow,
    CanonicalSubmitReceipt, CanonicalSubmitReceiptValidationError, CommitValidationError,
    ConsistencyWindowValidationError, DefinitionMigrationReceipt,
    DefinitionMigrationReceiptValidationError, DefinitionRegistrationReceipt,
    DefinitionRegistrationReceiptValidationError, DiagnosticClass, DiagnosticSink,
    DiagnosticValidationError, EffectDispatchRequest, EffectReconciliationValidationError,
    ExternalEffectDisposition, ExternalEffectEvidence, ExternalEffectExecutor, ExternalEffectState,
    InboxAcceptanceReceipt, InboxReceiptValidationError, LeaseTokenSource, MAX_OUTBOX_CLAIM_BATCH,
    MAX_OUTBOX_DELIVERY_ATTEMPTS, MAX_OUTCOMES_PER_READ_PAGE, MAX_QUOTA_CAPACITY,
    MAX_REDACTED_DIAGNOSTIC_BYTES, ManualReviewAuditEntry, ManualReviewAuditValidationError,
    ManualReviewClaim, ManualReviewControl, ManualReviewDecision, ManualReviewOperation,
    ManualReviewReceipt, ManualReviewReceiptValidationError, ManualReviewResolution,
    ManualReviewValidationError, OutboxAcknowledgement, OutboxClaimRequest, OutboxLease,
    OutboxLeaseToken, OutboxRecord, OutboxStore, OutcomeIdSource, OutcomeLog,
    OutcomeLogValidationError, OutcomePageValidationError, OutcomeReplayPage, OutcomeReplayRequest,
    ProcessAuthorizationDecision, ProcessAuthorizationOperation, ProcessAuthorizationRequest,
    QuotaKind, QuotaRequest, QuotaValidationError, ReconciliationValidationError,
    RecoveryDisposition, RedactedDiagnostic, TimerClaimRequest, TimerClaimStore, TimerLease,
    TimerSchedule, TimerValidationError,
};
/// Compatibility exports for existing saga consumers.
pub use saga::*;
/// External canonical-evidence integrations.
pub mod integration;

/// Compatibility path for the StateChronicle adapter boundary.
pub use integration::statechronicle;
/// Frequently used StateChronicle committed-event correlation contracts.
pub use penelope_statechronicle::{
    CanonicalCommandExpectation, CanonicalCommitBindingError, CorrelationError,
    VerifiedCanonicalEvent, bind_verified_event_to_commit, verify_committed_event,
};

/// Recoverable task contracts, deterministic control and hosted supervision.
pub mod task;
/// Compatibility path for hosted task supervision.
pub use task::TaskSupervisor;

/// Optional rewindable computation API; no timeline dependency by default.
#[cfg(feature = "timeline")]
pub use penelope_timeline as timeline;

/// Durable storage contracts; adapters retain their protocol-specific semantics.
pub mod storage;

/// Runtime integration contracts for recoverable task execution.
pub mod runtime;

/// Typed application machines; no storage or runtime is required.
pub use penelope_core::machine;
