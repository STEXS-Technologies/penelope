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
    ActionId, CanonicalCommandDtoV1, CanonicalEventDtoV1, ContentDigest, DefinitionId,
    DefinitionVersion, DomainError, EffectKeyV1, LogicalTimeV1, OutcomeActorV1, ProcessActionDtoV1,
    ProcessActionKindV1, ProcessId, ProcessInputEnvelopeV1, ProcessOutcomeDtoV1,
    ProcessOutcomeFactV1, ProcessScopeV1, StepId, TenantId,
};
/// Application execution boundary.
pub use penelope_executor as executor;
/// Deterministic linear-saga reference API.
pub use penelope_executor::engine::{
    ActionResultObservationV1, ActionResultV1, CompensationPlanV1, EngineError,
    LinearSagaDefinitionV1, LinearSagaEventEnvelopeV1, LinearSagaEventV1, LinearSagaInputV1,
    LinearSagaProjectionV1, RetryBackoffV1, RetryPolicyV1, SagaDecisionV1, SagaStatusV1,
    StepPlanV1, apply_action_result, decide, replay, replay_ordered, start,
};
/// Inbound parsing and validation boundary.
pub use penelope_intent as intent;
/// User-facing typed process-input parser.
pub use penelope_intent::{IntentError, parse_process_input};
/// Backend-neutral ports implemented by the composition root.
pub use penelope_ports as ports;
/// Frequently used typed process-control port contracts.
pub use penelope_ports::{
    ProcessAuthorizationDecisionV1, ProcessAuthorizationOperationV1, ProcessAuthorizationRequestV1,
    TimerScheduleV1,
};
/// StateChronicle adapter boundary; no client implementation is included.
pub use penelope_statechronicle as statechronicle;
