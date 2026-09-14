//! Versioned, validated DTOs for Penelope's public and adapter boundaries.
//!
//! Application and port APIs use these typed values rather than raw strings.
//! Text is admitted only through explicit parse/deserialization constructors.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use core::fmt;
use core::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Maximum length of a textual protocol identifier.
pub const MAX_IDENTIFIER_LENGTH: usize = 128;
/// Maximum steps in one immutable process definition.
pub const MAX_DEFINITION_STEPS: usize = 128;
/// Maximum total action attempts allowed for one declared step.
pub const MAX_ACTION_ATTEMPTS_PER_STEP: u32 = 64;
/// Maximum resources a single canonical command or event may scope.
pub const MAX_CANONICAL_RESOURCE_IDS: usize = 64;

/// Domain validation failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// A typed identifier did not have its required canonical form.
    #[error("invalid tenant identifier")]
    InvalidTenantId,
    /// A process identifier did not have its required canonical form.
    #[error("invalid process identifier")]
    InvalidProcessId,
    /// A definition identifier did not have its required canonical form.
    #[error("invalid definition identifier")]
    InvalidDefinitionId,
    /// A definition-version identifier did not have its required canonical form.
    #[error("invalid definition version identifier")]
    InvalidDefinitionVersion,
    /// A step identifier did not have its required canonical form.
    #[error("invalid step identifier")]
    InvalidStepId,
    /// An input identifier did not have its required canonical form.
    #[error("invalid input identifier")]
    InvalidInputId,
    /// An outcome identifier did not have its required canonical form.
    #[error("invalid outcome identifier")]
    InvalidOutcomeId,
    /// An action identifier did not have its required canonical form.
    #[error("invalid action identifier")]
    InvalidActionId,
    /// A review identifier did not have its required canonical form.
    #[error("invalid review identifier")]
    InvalidReviewId,
    /// A principal identifier did not have its required canonical form.
    #[error("invalid principal identifier")]
    InvalidPrincipalId,
    /// A canonical-event identifier did not have its required canonical form.
    #[error("invalid canonical event identifier")]
    InvalidCanonicalEventId,
    /// A canonical-commit identifier did not have its required canonical form.
    #[error("invalid canonical commit identifier")]
    InvalidCanonicalCommitId,
    /// A resource identifier did not have its required canonical form.
    #[error("invalid resource identifier")]
    InvalidResourceId,
    /// An operation identifier did not have its required canonical form.
    #[error("invalid operation identifier")]
    InvalidOperationId,
    /// An external-effect reference did not have its required canonical form.
    #[error("invalid external effect reference identifier")]
    InvalidExternalReferenceId,
    /// A process definition has no executable steps.
    #[error("process definition contains no steps")]
    EmptyDefinitionSteps,
    /// A process definition exceeds the bounded step limit.
    #[error("process definition exceeds the step limit")]
    DefinitionStepLimitExceeded,
    /// A process definition declares the same step identity more than once.
    #[error("process definition contains a duplicate step identifier")]
    DuplicateStepId,
    /// A canonical operation scope exceeds the bounded resource limit.
    #[error("canonical resource scope exceeds the resource limit")]
    CanonicalResourceLimitExceeded,
    /// A canonical operation scope contains the same resource more than once.
    #[error("canonical resource scope contains a duplicate resource identifier")]
    DuplicateCanonicalResourceId,
    /// A process-input envelope declared a schema for a different DTO type.
    #[error("process input envelope schema is invalid")]
    InvalidProcessInputSchema,
    /// A process-definition record declared a schema for a different DTO type.
    #[error("process definition record schema is invalid")]
    InvalidProcessDefinitionSchema,
    /// A process-outcome record declared a schema for a different DTO type.
    #[error("process outcome record schema is invalid")]
    InvalidProcessOutcomeSchema,
    /// A process-action record declared a schema for a different DTO type.
    #[error("process action record schema is invalid")]
    InvalidProcessActionSchema,
    /// A canonical command declared a schema for a different DTO type.
    #[error("canonical command schema is invalid")]
    InvalidCanonicalCommandSchema,
    /// A canonical event declared a schema for a different DTO type.
    #[error("canonical event schema is invalid")]
    InvalidCanonicalEventSchema,
    /// A manual-review record declared a schema for a different DTO type.
    #[error("manual review schema is invalid")]
    InvalidManualReviewSchema,
}

fn validate_identifier(prefix: &str, value: &str) -> bool {
    value.starts_with(prefix)
        && value.len() > prefix.len()
        && value.len() <= MAX_IDENTIFIER_LENGTH
        && value[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

macro_rules! identifier {
    ($name:ident, $prefix:literal, $error_variant:ident, $docs:literal) => {
        #[doc = $docs]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Parses and validates the canonical prefixed identifier.
            ///
            /// # Errors
            ///
            /// Returns a typed [`DomainError`] variant when the required
            /// prefix is absent, the body is empty, too long, or contains a
            /// control character.
            pub fn new(value: String) -> Result<Self, DomainError> {
                if validate_identifier($prefix, &value) {
                    Ok(Self(value))
                } else {
                    Err(DomainError::$error_variant)
                }
            }

            /// Returns the canonical wire representation.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(String::from(value))
            }
        }

        impl TryFrom<String> for $name {
            type Error = DomainError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = DomainError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(String::from(value))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(D::Error::custom)
            }
        }
    };
}

identifier!(
    TenantId,
    "tnt_",
    InvalidTenantId,
    "Isolated tenant identity."
);
identifier!(
    ProcessId,
    "prc_",
    InvalidProcessId,
    "Process-instance identity."
);
identifier!(
    DefinitionId,
    "def_",
    InvalidDefinitionId,
    "Process-definition identity."
);
identifier!(
    DefinitionVersion,
    "dfv_",
    InvalidDefinitionVersion,
    "Pinned definition version identity."
);
identifier!(StepId, "stp_", InvalidStepId, "Definition step identity.");
identifier!(
    InputId,
    "inp_",
    InvalidInputId,
    "Immutable inbox input identity."
);
identifier!(
    OutcomeId,
    "out_",
    InvalidOutcomeId,
    "Immutable outcome identity."
);
identifier!(
    ActionId,
    "act_",
    InvalidActionId,
    "Stable action and idempotency identity."
);
identifier!(ReviewId, "rev_", InvalidReviewId, "Manual-review identity.");
identifier!(
    PrincipalId,
    "pri_",
    InvalidPrincipalId,
    "Authorized actor identity."
);
identifier!(
    CanonicalEventId,
    "cev_",
    InvalidCanonicalEventId,
    "Canonical source-event identity."
);
identifier!(
    CanonicalCommitId,
    "cmt_",
    InvalidCanonicalCommitId,
    "Canonical commit identity."
);
identifier!(
    ResourceId,
    "res_",
    InvalidResourceId,
    "Canonical resource-scope identity."
);
identifier!(
    OperationId,
    "op_",
    InvalidOperationId,
    "Registered canonical operation identity."
);
identifier!(
    ExternalReferenceId,
    "ext_",
    InvalidExternalReferenceId,
    "External executor or remote-system reference identity."
);

/// Fixed-size digest of canonical payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentDigest(pub [u8; 32]);

/// A deterministic millisecond timestamp supplied by an injected clock.
///
/// The pure engine must never read wall clock time directly; it receives this
/// typed value through an application command or port response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LogicalTimeV1(pub u64);

/// The supported schema for a public DTO envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaV1 {
    /// `penelope.process.definition.v1`.
    #[serde(rename = "penelope.process.definition.v1")]
    ProcessDefinition,
    /// `penelope.process.input.v1`.
    #[serde(rename = "penelope.process.input.v1")]
    ProcessInput,
    /// `penelope.process.outcome.v1`.
    #[serde(rename = "penelope.process.outcome.v1")]
    ProcessOutcome,
    /// `penelope.process.action.v1`.
    #[serde(rename = "penelope.process.action.v1")]
    ProcessAction,
    /// `penelope.canonical.command.v1`.
    #[serde(rename = "penelope.canonical.command.v1")]
    CanonicalCommand,
    /// `penelope.canonical.event.v1`.
    #[serde(rename = "penelope.canonical.event.v1")]
    CanonicalEvent,
    /// `penelope.manual-review.v1`.
    #[serde(rename = "penelope.manual-review.v1")]
    ManualReview,
}

/// Typed process input category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessInputKindV1 {
    /// A verified committed canonical event.
    CanonicalEvent,
    /// A durable timer firing.
    TimerFired,
    /// An authorized manual-review decision.
    ManualResolution,
}

/// Typed append-only process outcome category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessOutcomeKindV1 {
    /// Process creation was recorded.
    Started,
    /// An input was durably accepted.
    InputAccepted,
    /// An action was planned.
    ActionPlanned,
    /// An action attempt started.
    ActionAttempted,
    /// A command committed canonically.
    CommandCommitted,
    /// An action completed successfully without asserting a canonical commit.
    ActionSucceeded,
    /// An action has a known terminal failure.
    ActionFailed,
    /// An action result is ambiguous and therefore cannot be retried blindly.
    ActionOutcomeUnknown,
    /// A retry was planned.
    RetryScheduled,
    /// A durable retry timer firing was accepted.
    TimerFired,
    /// A compensation action was planned.
    CompensationPlanned,
    /// Manual review was opened.
    ReviewOpened,
    /// An authorized manual-review decision was accepted.
    ManualResolutionApplied,
    /// Process completion was recorded.
    Completed,
    /// Process cancellation was recorded.
    Cancelled,
    /// All required compensations completed successfully.
    Compensated,
    /// Process escalation was recorded.
    Escalated,
}

/// Typed durable action category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessActionKindV1 {
    /// An idempotent StateChronicle command.
    CanonicalCommand,
    /// A timer schedule/cancel operation.
    Timer,
    /// An external effect requiring reconciliation.
    ExternalEffect,
    /// A manual-review escalation.
    ManualReview,
}

/// Typed causal identity of an outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CausationIdV1 {
    /// The outcome was caused by an accepted input.
    Input(InputId),
    /// The outcome was caused by an action.
    Action(ActionId),
}

/// Attributable source that recorded one immutable process outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutcomeActorV1 {
    /// The deterministic process runtime recorded the outcome.
    System,
    /// An authenticated principal recorded an authorized outcome.
    Principal(PrincipalId),
}

/// Attributable immutable facts recorded in one process outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutcomeFactV1 {
    /// Immutable idempotency identity for this outcome.
    pub outcome_id: OutcomeId,
    /// Typed causal input or action identity.
    pub causation_id: CausationIdV1,
    /// Attributable source that recorded this immutable fact.
    pub actor: OutcomeActorV1,
    /// Deterministic recorded-at time supplied by an injected clock.
    pub occurred_at: LogicalTimeV1,
    /// Typed outcome category.
    pub kind: ProcessOutcomeKindV1,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

impl ProcessOutcomeFactV1 {
    /// Groups the immutable fact fields for a process outcome record.
    pub const fn new(
        outcome_id: OutcomeId,
        causation_id: CausationIdV1,
        actor: OutcomeActorV1,
        occurred_at: LogicalTimeV1,
        kind: ProcessOutcomeKindV1,
        payload_digest: ContentDigest,
    ) -> Self {
        Self {
            outcome_id,
            causation_id,
            actor,
            occurred_at,
            kind,
            payload_digest,
        }
    }
}

/// A version-pinned immutable process definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDefinitionDtoV1 {
    /// The immutable schema discriminator for this record.
    pub schema: SchemaV1,
    /// Stable definition identity.
    pub definition_id: DefinitionId,
    /// Immutable definition version selected when an instance starts.
    pub definition_version: DefinitionVersion,
    /// Digest of the canonical definition representation.
    pub definition_digest: ContentDigest,
    /// Ordered declared step identifiers.
    pub step_ids: Vec<StepId>,
}

/// A causally attributable process input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInputDtoV1 {
    /// The immutable schema discriminator for this record.
    pub schema: SchemaV1,
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Target process instance.
    pub process_id: ProcessId,
    /// Immutable input identity used for inbox deduplication.
    pub input_id: InputId,
    /// Typed input category.
    pub kind: ProcessInputKindV1,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

/// Explicitly versioned public envelope for one process input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInputEnvelopeV1 {
    /// The immutable schema discriminator for the enclosed input.
    pub schema: SchemaV1,
    /// Typed process input supplied to the application boundary.
    pub input: ProcessInputDtoV1,
}

impl ProcessInputEnvelopeV1 {
    /// Creates an envelope carrying the only schema valid for this DTO.
    pub const fn new(input: ProcessInputDtoV1) -> Self {
        Self {
            schema: SchemaV1::ProcessInput,
            input,
        }
    }

    /// Validates the immutable envelope schema discriminator.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the envelope does not declare the process
    /// input schema.
    pub fn validate(&self) -> Result<(), DomainError> {
        self.input.validate()?;
        if !matches!(self.schema, SchemaV1::ProcessInput) {
            return Err(DomainError::InvalidProcessInputSchema);
        }
        Ok(())
    }
}

/// One immutable fact in a process outcome log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutcomeDtoV1 {
    /// The immutable schema discriminator for this record.
    pub schema: SchemaV1,
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Process instance that owns the outcome.
    pub process_id: ProcessId,
    /// Immutable definition identity pinned when this process started.
    pub definition_id: DefinitionId,
    /// Immutable definition version pinned when this process started.
    pub definition_version: DefinitionVersion,
    /// Digest of the exact pinned definition representation.
    pub definition_digest: ContentDigest,
    /// Strictly ordered per-instance outcome sequence.
    pub sequence: u64,
    /// Immutable outcome identity.
    pub outcome_id: OutcomeId,
    /// Typed causal input or action identity.
    pub causation_id: CausationIdV1,
    /// Attributable source that recorded this immutable fact.
    pub actor: OutcomeActorV1,
    /// Deterministic recorded-at time supplied by an injected clock.
    pub occurred_at: LogicalTimeV1,
    /// Typed outcome category.
    pub kind: ProcessOutcomeKindV1,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

/// Immutable identity and definition scope of one process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessScopeV1 {
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Owning process instance.
    pub process_id: ProcessId,
    /// Immutable definition identity selected for this process.
    pub definition_id: DefinitionId,
    /// Immutable definition version selected for this process.
    pub definition_version: DefinitionVersion,
    /// Exact definition semantics selected for this process.
    pub definition_digest: ContentDigest,
}

impl ProcessScopeV1 {
    /// Creates the immutable identity and definition scope of one process.
    pub const fn new(
        tenant_id: TenantId,
        process_id: ProcessId,
        definition_id: DefinitionId,
        definition_version: DefinitionVersion,
        definition_digest: ContentDigest,
    ) -> Self {
        Self {
            tenant_id,
            process_id,
            definition_id,
            definition_version,
            definition_digest,
        }
    }
}

/// A durable, independently idempotent process action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessActionDtoV1 {
    /// The immutable schema discriminator for this record.
    pub schema: SchemaV1,
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Owning process instance.
    pub process_id: ProcessId,
    /// Immutable definition identity selected for this process.
    pub definition_id: DefinitionId,
    /// Immutable definition version selected for this process.
    pub definition_version: DefinitionVersion,
    /// Exact definition semantics selected for this process.
    pub definition_digest: ContentDigest,
    /// Stable action identity and external idempotency key.
    pub action_id: ActionId,
    /// Pinned definition step identity.
    pub step_id: StepId,
    /// Attempt number for this action.
    pub attempt: u32,
    /// Typed action category.
    pub kind: ProcessActionKindV1,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

/// Stable semantic idempotency key for one process effect attempt.
///
/// Unlike [`ActionId`], this key is derived from the pinned process definition
/// and the effect's semantic coordinates. Adapters can use it to detect a
/// duplicate semantic dispatch without reconstructing an untyped string key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectKeyV1 {
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Owning process instance.
    pub process_id: ProcessId,
    /// Immutable definition identity selected for this process.
    pub definition_id: DefinitionId,
    /// Immutable definition version selected for this process.
    pub definition_version: DefinitionVersion,
    /// Exact definition semantics selected for this process.
    pub definition_digest: ContentDigest,
    /// Pinned definition step identity.
    pub step_id: StepId,
    /// Attempt number for this effect.
    pub attempt: u32,
    /// Typed external effect category.
    pub kind: ProcessActionKindV1,
    /// Canonical payload semantics for this effect attempt.
    pub payload_digest: ContentDigest,
}

/// A canonical-state command submitted through an adapter port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalCommandDtoV1 {
    /// The immutable schema discriminator for this record.
    pub schema: SchemaV1,
    /// Tenant authorized for the command.
    pub tenant_id: TenantId,
    /// Penelope action ID reused as canonical idempotency ID.
    pub action_id: ActionId,
    /// Registered typed canonical operation.
    pub operation: OperationId,
    /// Expected resource scope identifiers.
    pub resource_ids: Vec<ResourceId>,
    /// Canonical command payload digest.
    pub payload_digest: ContentDigest,
}

/// Evidence of a verified committed canonical-state result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalEventDtoV1 {
    /// The immutable schema discriminator for this record.
    pub schema: SchemaV1,
    /// Tenant of the committed event.
    pub tenant_id: TenantId,
    /// Immutable source delivery/event identity used by the inbox.
    pub source_event_id: CanonicalEventId,
    /// Expected Penelope action/canonical idempotency identity.
    pub action_id: ActionId,
    /// Canonical commit identity.
    pub commit_id: CanonicalCommitId,
    /// Canonical commit sequence.
    pub commit_sequence: u64,
    /// Registered typed canonical operation.
    pub operation: OperationId,
    /// Affected resource scope identifiers.
    pub resource_ids: Vec<ResourceId>,
    /// Canonical event payload digest.
    pub payload_digest: ContentDigest,
}

/// A durable request for authorized human resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualReviewDtoV1 {
    /// The immutable schema discriminator for this record.
    pub schema: SchemaV1,
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Owning process instance.
    pub process_id: ProcessId,
    /// Immutable definition identity selected for this process.
    pub definition_id: DefinitionId,
    /// Immutable definition version selected for this process.
    pub definition_version: DefinitionVersion,
    /// Exact definition semantics selected for this process.
    pub definition_digest: ContentDigest,
    /// Stable review identity.
    pub review_id: ReviewId,
    /// Outcome sequence at which review was opened.
    pub opened_at_sequence: u64,
    /// Optional inclusive logical deadline for an authorized review decision.
    pub expires_at: Option<LogicalTimeV1>,
    /// Redacted evidence digest.
    pub evidence_digest: ContentDigest,
}

impl ProcessDefinitionDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ProcessDefinition;

    /// Creates a version-pinned process definition DTO.
    ///
    /// # Errors
    ///
    /// Returns a typed error for empty, oversized, or duplicate step lists.
    pub fn new(
        definition_id: DefinitionId,
        definition_version: DefinitionVersion,
        definition_digest: ContentDigest,
        step_ids: Vec<StepId>,
    ) -> Result<Self, DomainError> {
        let definition = Self {
            schema: Self::SCHEMA,
            definition_id,
            definition_version,
            definition_digest,
            step_ids,
        };
        definition.validate()?;
        Ok(definition)
    }

    /// Validates bounded, unambiguous definition steps.
    ///
    /// # Errors
    ///
    /// Returns a typed error for empty, oversized, or duplicate step lists.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !matches!(self.schema, SchemaV1::ProcessDefinition) {
            return Err(DomainError::InvalidProcessDefinitionSchema);
        }
        if self.step_ids.is_empty() {
            return Err(DomainError::EmptyDefinitionSteps);
        }
        if self.step_ids.len() > MAX_DEFINITION_STEPS {
            return Err(DomainError::DefinitionStepLimitExceeded);
        }
        if self.step_ids.iter().enumerate().any(|(index, step_id)| {
            self.step_ids
                .iter()
                .skip(index.saturating_add(1))
                .any(|other_step_id| other_step_id == step_id)
        }) {
            return Err(DomainError::DuplicateStepId);
        }
        Ok(())
    }
}

impl ProcessInputDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ProcessInput;

    /// Creates a typed process input DTO.
    pub const fn new(
        tenant_id: TenantId,
        process_id: ProcessId,
        input_id: InputId,
        kind: ProcessInputKindV1,
        payload_digest: ContentDigest,
    ) -> Self {
        Self {
            schema: Self::SCHEMA,
            tenant_id,
            process_id,
            input_id,
            kind,
            payload_digest,
        }
    }

    /// Validates the immutable input schema discriminator.
    ///
    /// # Errors
    ///
    /// Returns a typed error when this record declares another DTO schema.
    pub const fn validate(&self) -> Result<(), DomainError> {
        if matches!(self.schema, SchemaV1::ProcessInput) {
            Ok(())
        } else {
            Err(DomainError::InvalidProcessInputSchema)
        }
    }
}

impl ProcessOutcomeDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ProcessOutcome;

    /// Creates an immutable process outcome DTO.
    pub fn new(scope: ProcessScopeV1, sequence: u64, fact: ProcessOutcomeFactV1) -> Self {
        Self {
            schema: Self::SCHEMA,
            tenant_id: scope.tenant_id,
            process_id: scope.process_id,
            definition_id: scope.definition_id,
            definition_version: scope.definition_version,
            definition_digest: scope.definition_digest,
            sequence,
            outcome_id: fact.outcome_id,
            causation_id: fact.causation_id,
            actor: fact.actor,
            occurred_at: fact.occurred_at,
            kind: fact.kind,
            payload_digest: fact.payload_digest,
        }
    }

    /// Validates the immutable outcome record schema discriminator.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the record declares a different DTO schema.
    pub const fn validate(&self) -> Result<(), DomainError> {
        if matches!(self.schema, SchemaV1::ProcessOutcome) {
            Ok(())
        } else {
            Err(DomainError::InvalidProcessOutcomeSchema)
        }
    }
}

impl ProcessActionDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ProcessAction;

    /// Creates a stable independently idempotent action DTO.
    pub fn new(
        scope: ProcessScopeV1,
        action_id: ActionId,
        step_id: StepId,
        attempt: u32,
        kind: ProcessActionKindV1,
        payload_digest: ContentDigest,
    ) -> Self {
        Self {
            schema: Self::SCHEMA,
            tenant_id: scope.tenant_id,
            process_id: scope.process_id,
            definition_id: scope.definition_id,
            definition_version: scope.definition_version,
            definition_digest: scope.definition_digest,
            action_id,
            step_id,
            attempt,
            kind,
            payload_digest,
        }
    }

    /// Validates the immutable action schema discriminator.
    ///
    /// # Errors
    ///
    /// Returns a typed error when this record declares another DTO schema.
    pub const fn validate(&self) -> Result<(), DomainError> {
        if matches!(self.schema, SchemaV1::ProcessAction) {
            Ok(())
        } else {
            Err(DomainError::InvalidProcessActionSchema)
        }
    }

    /// Returns the immutable process and definition scope for this action.
    #[must_use]
    pub fn scope(&self) -> ProcessScopeV1 {
        ProcessScopeV1::new(
            self.tenant_id.clone(),
            self.process_id.clone(),
            self.definition_id.clone(),
            self.definition_version.clone(),
            self.definition_digest,
        )
    }

    /// Derives the stable semantic idempotency key for this action attempt.
    pub fn effect_key(&self) -> EffectKeyV1 {
        EffectKeyV1 {
            tenant_id: self.tenant_id.clone(),
            process_id: self.process_id.clone(),
            definition_id: self.definition_id.clone(),
            definition_version: self.definition_version.clone(),
            definition_digest: self.definition_digest,
            step_id: self.step_id.clone(),
            attempt: self.attempt,
            kind: self.kind,
            payload_digest: self.payload_digest,
        }
    }
}

impl CanonicalCommandDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::CanonicalCommand;

    /// Creates a typed canonical command DTO.
    ///
    /// # Errors
    ///
    /// Returns a typed error for oversized or duplicate resource identifiers.
    pub fn new(
        tenant_id: TenantId,
        action_id: ActionId,
        operation: OperationId,
        resource_ids: Vec<ResourceId>,
        payload_digest: ContentDigest,
    ) -> Result<Self, DomainError> {
        let command = Self {
            schema: Self::SCHEMA,
            tenant_id,
            action_id,
            operation,
            resource_ids,
            payload_digest,
        };
        command.validate()?;
        Ok(command)
    }

    /// Validates the bounded, unambiguous canonical resource scope.
    ///
    /// # Errors
    ///
    /// Returns a typed error for oversized or duplicate resource identifiers.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !matches!(self.schema, SchemaV1::CanonicalCommand) {
            return Err(DomainError::InvalidCanonicalCommandSchema);
        }
        validate_canonical_resource_scope(&self.resource_ids)
    }
}

impl CanonicalEventDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::CanonicalEvent;

    /// Creates a typed committed canonical event.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an oversized or duplicate resource scope.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tenant_id: TenantId,
        source_event_id: CanonicalEventId,
        action_id: ActionId,
        commit_id: CanonicalCommitId,
        commit_sequence: u64,
        operation: OperationId,
        resource_ids: Vec<ResourceId>,
        payload_digest: ContentDigest,
    ) -> Result<Self, DomainError> {
        let event = Self {
            schema: Self::SCHEMA,
            tenant_id,
            source_event_id,
            action_id,
            commit_id,
            commit_sequence,
            operation,
            resource_ids,
            payload_digest,
        };
        event.validate()?;
        Ok(event)
    }

    /// Validates the bounded, unambiguous canonical resource scope.
    ///
    /// # Errors
    ///
    /// Returns a typed error for oversized or duplicate resource identifiers.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !matches!(self.schema, SchemaV1::CanonicalEvent) {
            return Err(DomainError::InvalidCanonicalEventSchema);
        }
        validate_canonical_resource_scope(&self.resource_ids)
    }
}

/// Validates a canonical operation's bounded, duplicate-free resource scope.
///
/// # Errors
///
/// Returns a typed error for an oversized scope or duplicate resource identity.
pub fn validate_canonical_resource_scope(resource_ids: &[ResourceId]) -> Result<(), DomainError> {
    if resource_ids.len() > MAX_CANONICAL_RESOURCE_IDS {
        return Err(DomainError::CanonicalResourceLimitExceeded);
    }
    if resource_ids.iter().enumerate().any(|(index, resource_id)| {
        resource_ids
            .iter()
            .skip(index.saturating_add(1))
            .any(|other_resource_id| other_resource_id == resource_id)
    }) {
        return Err(DomainError::DuplicateCanonicalResourceId);
    }
    Ok(())
}

impl ManualReviewDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ManualReview;

    /// Creates a durable manual-review escalation DTO.
    pub fn new(
        scope: ProcessScopeV1,
        review_id: ReviewId,
        opened_at_sequence: u64,
        expires_at: Option<LogicalTimeV1>,
        evidence_digest: ContentDigest,
    ) -> Self {
        Self {
            schema: Self::SCHEMA,
            tenant_id: scope.tenant_id,
            process_id: scope.process_id,
            definition_id: scope.definition_id,
            definition_version: scope.definition_version,
            definition_digest: scope.definition_digest,
            review_id,
            opened_at_sequence,
            expires_at,
            evidence_digest,
        }
    }

    /// Validates the immutable manual-review schema discriminator.
    ///
    /// # Errors
    ///
    /// Returns a typed error when this record declares another DTO schema.
    pub const fn validate(&self) -> Result<(), DomainError> {
        if matches!(self.schema, SchemaV1::ManualReview) {
            Ok(())
        } else {
            Err(DomainError::InvalidManualReviewSchema)
        }
    }

    /// Returns the immutable process and definition scope for this review.
    #[must_use]
    pub fn scope(&self) -> ProcessScopeV1 {
        ProcessScopeV1::new(
            self.tenant_id.clone(),
            self.process_id.clone(),
            self.definition_id.clone(),
            self.definition_version.clone(),
            self.definition_digest,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    #[test]
    fn invalid_identifiers_report_their_typed_error_variant() {
        assert_eq!(
            TenantId::try_from("prc_wrong").unwrap_err(),
            DomainError::InvalidTenantId
        );
        assert_eq!(
            PrincipalId::try_from("tnt_wrong").unwrap_err(),
            DomainError::InvalidPrincipalId
        );
    }

    #[test]
    fn identifiers_reject_ambiguous_storage_and_log_characters() {
        for value in [
            "tnt_with space",
            "tnt_with/slash",
            "tnt_with\\\\slash",
            "tnt_ümlaut",
        ] {
            assert_eq!(
                TenantId::try_from(value).unwrap_err(),
                DomainError::InvalidTenantId
            );
        }
        assert!(TenantId::try_from("tnt_safe-id.v1").is_ok());
    }

    #[test]
    fn effect_key_binds_every_semantic_effect_coordinate() {
        let scope = ProcessScopeV1::new(
            id::<TenantId>("tnt_game"),
            id::<ProcessId>("prc_trade"),
            id::<DefinitionId>("def_trade"),
            id::<DefinitionVersion>("dfv_one"),
            ContentDigest([1; 32]),
        );
        let action = ProcessActionDtoV1::new(
            scope.clone(),
            id::<ActionId>("act_first"),
            id::<StepId>("stp_settle"),
            0,
            ProcessActionKindV1::CanonicalCommand,
            ContentDigest([2; 32]),
        );
        let same_semantics_new_action_id = ProcessActionDtoV1::new(
            scope,
            id::<ActionId>("act_second"),
            id::<StepId>("stp_settle"),
            0,
            ProcessActionKindV1::CanonicalCommand,
            ContentDigest([2; 32]),
        );
        let retry = ProcessActionDtoV1::new(
            ProcessScopeV1::new(
                id::<TenantId>("tnt_game"),
                id::<ProcessId>("prc_trade"),
                id::<DefinitionId>("def_trade"),
                id::<DefinitionVersion>("dfv_one"),
                ContentDigest([1; 32]),
            ),
            id::<ActionId>("act_retry"),
            id::<StepId>("stp_settle"),
            1,
            ProcessActionKindV1::CanonicalCommand,
            ContentDigest([3; 32]),
        );
        let changed_payload = ProcessActionDtoV1::new(
            ProcessScopeV1::new(
                id::<TenantId>("tnt_game"),
                id::<ProcessId>("prc_trade"),
                id::<DefinitionId>("def_trade"),
                id::<DefinitionVersion>("dfv_one"),
                ContentDigest([1; 32]),
            ),
            id::<ActionId>("act_changed_payload"),
            id::<StepId>("stp_settle"),
            0,
            ProcessActionKindV1::CanonicalCommand,
            ContentDigest([4; 32]),
        );

        assert_eq!(
            action.effect_key(),
            same_semantics_new_action_id.effect_key()
        );
        assert_ne!(action.effect_key(), retry.effect_key());
        assert_ne!(action.effect_key(), changed_payload.effect_key());
    }

    #[test]
    fn deserialization_preserves_identifier_validation() {
        let error = serde_json::from_str::<ActionId>("\"not-an-action\"").unwrap_err();
        assert!(error.is_data());
    }

    #[test]
    fn schema_wire_discriminators_are_canonical_versioned_protocol_values() {
        assert_eq!(
            serde_json::to_string(&SchemaV1::ProcessInput).unwrap(),
            format!("\"{}\"", penelope_core::schema::PROCESS_INPUT_V1)
        );
        assert_eq!(
            serde_json::to_string(&SchemaV1::ProcessOutcome).unwrap(),
            format!("\"{}\"", penelope_core::schema::PROCESS_OUTCOME_V1)
        );
        assert!(serde_json::from_str::<SchemaV1>("\"penelope.process.input.v2\"").is_err());
        assert!(serde_json::from_str::<SchemaV1>("\"ProcessInput\"").is_err());
    }

    #[test]
    fn every_public_wire_dto_rejects_a_mismatched_schema() {
        let mut definition = ProcessDefinitionDtoV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([0; 32]),
            vec![id("stp_lock")],
        )
        .unwrap();
        definition.schema = SchemaV1::ProcessInput;
        assert_eq!(
            definition.validate(),
            Err(DomainError::InvalidProcessDefinitionSchema)
        );

        let mut input = ProcessInputDtoV1::new(
            id("tnt_market"),
            id("prc_trade"),
            id("inp_event"),
            ProcessInputKindV1::CanonicalEvent,
            ContentDigest([1; 32]),
        );
        input.schema = SchemaV1::ProcessOutcome;
        assert_eq!(
            input.validate(),
            Err(DomainError::InvalidProcessInputSchema)
        );

        let scope = ProcessScopeV1::new(
            id("tnt_market"),
            id("prc_trade"),
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([2; 32]),
        );
        let mut outcome = ProcessOutcomeDtoV1::new(
            scope.clone(),
            0,
            ProcessOutcomeFactV1::new(
                id("out_started"),
                CausationIdV1::Input(id("inp_event")),
                OutcomeActorV1::System,
                LogicalTimeV1(0),
                ProcessOutcomeKindV1::Started,
                ContentDigest([3; 32]),
            ),
        );
        outcome.schema = SchemaV1::ProcessAction;
        assert_eq!(
            outcome.validate(),
            Err(DomainError::InvalidProcessOutcomeSchema)
        );

        let mut action = ProcessActionDtoV1::new(
            scope,
            id("act_lock"),
            id("stp_lock"),
            0,
            ProcessActionKindV1::CanonicalCommand,
            ContentDigest([4; 32]),
        );
        action.schema = SchemaV1::CanonicalCommand;
        assert_eq!(
            action.validate(),
            Err(DomainError::InvalidProcessActionSchema)
        );

        let mut command = CanonicalCommandDtoV1::new(
            id("tnt_market"),
            id("act_lock"),
            id("op_lock"),
            vec![id("res_asset")],
            ContentDigest([5; 32]),
        )
        .unwrap();
        command.schema = SchemaV1::CanonicalEvent;
        assert_eq!(
            command.validate(),
            Err(DomainError::InvalidCanonicalCommandSchema)
        );

        let mut event = CanonicalEventDtoV1::new(
            id("tnt_market"),
            id("cev_event"),
            id("act_lock"),
            id("cmt_commit"),
            0,
            id("op_lock"),
            vec![id("res_asset")],
            ContentDigest([6; 32]),
        )
        .unwrap();
        event.schema = SchemaV1::ManualReview;
        assert_eq!(
            event.validate(),
            Err(DomainError::InvalidCanonicalEventSchema)
        );

        let mut review = ManualReviewDtoV1::new(
            ProcessScopeV1::new(
                id("tnt_market"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([4; 32]),
            ),
            id("rev_trade"),
            0,
            None,
            ContentDigest([7; 32]),
        );
        review.schema = SchemaV1::ProcessDefinition;
        assert_eq!(
            review.validate(),
            Err(DomainError::InvalidManualReviewSchema)
        );
    }

    #[test]
    fn definition_validation_bounds_and_deduplicates_steps() {
        let oversized = ProcessDefinitionDtoV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([0; 32]),
            vec![id::<StepId>("stp_lock"); MAX_DEFINITION_STEPS.saturating_add(1)],
        );
        assert_eq!(
            oversized.unwrap_err(),
            DomainError::DefinitionStepLimitExceeded
        );

        let duplicate = ProcessDefinitionDtoV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([0; 32]),
            vec![id("stp_lock"), id("stp_lock")],
        );
        assert_eq!(duplicate.unwrap_err(), DomainError::DuplicateStepId);
    }

    #[test]
    fn canonical_scope_validation_bounds_and_deduplicates_resources() {
        let duplicate = CanonicalCommandDtoV1::new(
            id("tnt_market"),
            id("act_settle"),
            id("op_settle"),
            vec![id::<ResourceId>("res_asset"), id("res_asset")],
            ContentDigest([0; 32]),
        );
        assert_eq!(
            duplicate.unwrap_err(),
            DomainError::DuplicateCanonicalResourceId
        );
    }
}
