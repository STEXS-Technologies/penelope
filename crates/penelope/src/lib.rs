//! Penelope umbrella crate.
//!
//! This is the consumer-facing facade for Penelope's hexagonal architecture:
//! pure protocol primitives, typed protocol values, intent boundary, deterministic
//! executor, backend-neutral ports, and adapter boundaries. It ships no
//! database, broker, scheduler, HTTP, or StateChronicle client implementation.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

/// Pure protocol primitives and schema identifiers.
pub use penelope_core as core;
/// Versioned data-transfer objects.
pub use penelope_domain as domain;
/// Frequently used typed protocol values and typed protocol values.
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
/// Deterministic linear-saga reference API.
pub use penelope_executor::engine::{
    ActionResult, ActionResultObservation, CompensationPlan, EngineError, GraphDefinitionError,
    GraphTransition, GraphTransitionOutcome, LinearSagaDefinition, LinearSagaEvent,
    LinearSagaEventEnvelope, LinearSagaInput, LinearSagaProjection, MAX_GRAPH_TRANSITIONS,
    ProcessGraphDefinition, RetryBackoff, RetryJitter, RetryJitterSeed, RetryPolicy,
    RetryTimerScheduleRequest, SagaDecision, SagaStatus, StepPlan, apply_action_result,
    apply_manual_resolution, decide, fire_retry_timer, replay, replay_ordered,
    schedule_retry_timer, start,
};
/// Deterministic bounded graph-process API.
pub use penelope_executor::graph::{
    GraphEngineError, GraphSagaDecision, GraphSagaEvent, GraphSagaEventEnvelope, GraphSagaInput,
    GraphSagaProjection, apply_graph_result, replay_graph, replay_graph_ordered, start_graph,
};
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
/// StateChronicle adapter boundary; no client implementation is included.
pub use penelope_statechronicle as statechronicle;
/// Frequently used StateChronicle committed-event correlation contracts.
pub use penelope_statechronicle::{
    CanonicalCommandExpectation, CanonicalCommitBindingError, CorrelationError,
    VerifiedCanonicalEvent, bind_verified_event_to_commit, verify_committed_event,
};
