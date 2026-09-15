//! Penelope umbrella crate.
//!
//! This is the consumer-facing facade for Penelope's hexagonal architecture:
//! pure protocol primitives, versioned DTOs, intent boundary, deterministic
//! executor, backend-neutral ports, and adapter boundaries. It ships no
//! database, broker, scheduler, HTTP, or StateChronicle client implementation.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

/// Pure protocol primitives and schema identifiers.
pub use penelope_core as core;
/// Versioned data-transfer objects.
pub use penelope_domain as domain;
/// Frequently used typed protocol values and versioned DTOs.
pub use penelope_domain::{
    ActionId, CanonicalCommandDtoV1, CanonicalCommitId, CanonicalEncodingError,
    CanonicalEventDtoV1, CanonicalEventId, CanonicalWireBytesV1, CausationIdV1, ContentDigest,
    DefinitionCompatibilityError, DefinitionCompatibilityV1, DefinitionId,
    DefinitionMigrationError, DefinitionMigrationV1, DefinitionVersion, DomainError, EffectKeyV1,
    ExternalReferenceId, InputId, LogicalTimeV1, ManualReviewDtoV1, MigrationId, OperationId,
    OutcomeActorV1, OutcomeId, ProcessActionDtoV1, ProcessActionKindV1, ProcessDefinitionDtoV1,
    ProcessId, ProcessInputDtoV1, ProcessInputEnvelopeV1, ProcessInputKindV1, ProcessOutcomeDtoV1,
    ProcessOutcomeFactV1, ProcessOutcomeKindV1, ProcessScopeV1, ResourceId, ReviewId, StepId,
    TenantId,
};
/// Application execution boundary.
pub use penelope_executor as executor;
/// Deterministic linear-saga reference API.
pub use penelope_executor::engine::{
    ActionResultObservationV1, ActionResultV1, CompensationPlanV1, EngineError,
    GraphDefinitionError, GraphTransitionOutcomeV1, GraphTransitionV1, LinearSagaDefinitionV1,
    LinearSagaEventEnvelopeV1, LinearSagaEventV1, LinearSagaInputV1, LinearSagaProjectionV1,
    MAX_GRAPH_TRANSITIONS, ProcessGraphDefinitionV1, RetryBackoffV1, RetryJitterSeedV1,
    RetryJitterV1, RetryPolicyV1, RetryTimerScheduleRequestV1, SagaDecisionV1, SagaStatusV1,
    StepPlanV1, apply_action_result, apply_manual_resolution, decide, fire_retry_timer, replay,
    replay_ordered, schedule_retry_timer, start,
};
/// Deterministic bounded graph-process API.
pub use penelope_executor::graph::{
    GraphEngineError, GraphSagaDecisionV1, GraphSagaEventEnvelopeV1, GraphSagaEventV1,
    GraphSagaInputV1, GraphSagaProjectionV1, apply_graph_result, replay_graph,
    replay_graph_ordered, start_graph,
};
/// Inbound parsing and validation boundary.
pub use penelope_intent as intent;
/// User-facing typed process-input parser.
pub use penelope_intent::{IntentError, parse_process_input};
/// Backend-neutral ports implemented by the composition root.
pub use penelope_ports as ports;
/// Frequently used typed process-control port contracts.
pub use penelope_ports::{
    ActionDispatchReceiptV1, ActionReceiptValidationError, AtomicProcessCommitReceiptV1,
    AtomicProcessCommitReceiptValidationError, AtomicProcessCommitV1, AuthorizationRequirementV1,
    CanonicalReconciliationV1, CanonicalReconciliationWindowV1, CanonicalSubmitReceiptV1,
    CanonicalSubmitReceiptValidationError, CommitValidationError, ConsistencyWindowValidationError,
    DefinitionMigrationReceiptV1, DefinitionMigrationReceiptValidationError,
    DefinitionRegistrationReceiptV1, DefinitionRegistrationReceiptValidationError,
    DiagnosticClassV1, DiagnosticSink, DiagnosticValidationError, EffectDispatchRequestV1,
    EffectReconciliationValidationError, ExternalEffectEvidenceV1, ExternalEffectExecutor,
    ExternalEffectStateV1, InboxAcceptanceReceiptV1, InboxReceiptValidationError, LeaseTokenSource,
    MAX_OUTBOX_CLAIM_BATCH, MAX_OUTBOX_DELIVERY_ATTEMPTS, MAX_OUTCOMES_PER_READ_PAGE,
    MAX_QUOTA_CAPACITY, MAX_REDACTED_DIAGNOSTIC_BYTES, ManualReviewClaimV1, ManualReviewControlV1,
    ManualReviewDecisionV1, ManualReviewReceiptV1, ManualReviewReceiptValidationError,
    ManualReviewResolutionV1, ManualReviewValidationError, OutboxAcknowledgementV1,
    OutboxClaimRequestV1, OutboxLeaseTokenV1, OutboxLeaseV1, OutboxRecordV1, OutboxStore,
    OutcomeIdSource, OutcomeLogV1, OutcomeLogValidationError, OutcomePageValidationError,
    OutcomeReplayPageV1, OutcomeReplayRequestV1, ProcessAuthorizationDecisionV1,
    ProcessAuthorizationOperationV1, ProcessAuthorizationRequestV1, QuotaKindV1, QuotaRequestV1,
    QuotaValidationError, ReconciliationValidationError, RedactedDiagnosticV1, TimerClaimRequestV1,
    TimerClaimStore, TimerLeaseV1, TimerScheduleV1, TimerValidationError,
};
/// StateChronicle adapter boundary; no client implementation is included.
pub use penelope_statechronicle as statechronicle;
/// Frequently used StateChronicle committed-event correlation contracts.
pub use penelope_statechronicle::{
    CanonicalCommandExpectationV1, CanonicalCommitBindingError, CorrelationError,
    VerifiedCanonicalEventV1, bind_verified_event_to_commit, verify_committed_event,
};
