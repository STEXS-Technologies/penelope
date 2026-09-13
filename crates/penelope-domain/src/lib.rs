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
}

fn validate_identifier(prefix: &str, value: &str) -> bool {
    value.starts_with(prefix)
        && value.len() > prefix.len()
        && value.len() <= MAX_IDENTIFIER_LENGTH
        && !value.chars().any(char::is_control)
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

/// Fixed-size digest of canonical payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentDigest(pub [u8; 32]);

/// The supported schema for a public DTO envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaV1 {
    /// `penelope.process.definition.v1`.
    ProcessDefinition,
    /// `penelope.process.input.v1`.
    ProcessInput,
    /// `penelope.process.outcome.v1`.
    ProcessOutcome,
    /// `penelope.process.action.v1`.
    ProcessAction,
    /// `penelope.canonical.command.v1`.
    CanonicalCommand,
    /// `penelope.canonical.event.v1`.
    CanonicalEvent,
    /// `penelope.manual-review.v1`.
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
    /// An action has a known terminal failure.
    ActionFailed,
    /// A retry was planned.
    RetryScheduled,
    /// A compensation action was planned.
    CompensationPlanned,
    /// Manual review was opened.
    ReviewOpened,
    /// Process completion was recorded.
    Completed,
    /// Process cancellation was recorded.
    Cancelled,
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

/// A version-pinned immutable process definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDefinitionDtoV1 {
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

/// One immutable fact in a process outcome log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutcomeDtoV1 {
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Process instance that owns the outcome.
    pub process_id: ProcessId,
    /// Strictly ordered per-instance outcome sequence.
    pub sequence: u64,
    /// Immutable outcome identity.
    pub outcome_id: OutcomeId,
    /// Typed causal input or action identity.
    pub causation_id: CausationIdV1,
    /// Typed outcome category.
    pub kind: ProcessOutcomeKindV1,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

/// A durable, independently idempotent process action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessActionDtoV1 {
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Owning process instance.
    pub process_id: ProcessId,
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

/// A canonical-state command submitted through an adapter port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalCommandDtoV1 {
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
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Owning process instance.
    pub process_id: ProcessId,
    /// Stable review identity.
    pub review_id: ReviewId,
    /// Outcome sequence at which review was opened.
    pub opened_at_sequence: u64,
    /// Redacted evidence digest.
    pub evidence_digest: ContentDigest,
}

impl ProcessDefinitionDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ProcessDefinition;

    /// Creates a version-pinned process definition DTO.
    pub const fn new(
        definition_id: DefinitionId,
        definition_version: DefinitionVersion,
        definition_digest: ContentDigest,
        step_ids: Vec<StepId>,
    ) -> Self {
        Self {
            definition_id,
            definition_version,
            definition_digest,
            step_ids,
        }
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
            tenant_id,
            process_id,
            input_id,
            kind,
            payload_digest,
        }
    }
}

impl ProcessOutcomeDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ProcessOutcome;

    /// Creates an immutable process outcome DTO.
    pub const fn new(
        tenant_id: TenantId,
        process_id: ProcessId,
        sequence: u64,
        outcome_id: OutcomeId,
        causation_id: CausationIdV1,
        kind: ProcessOutcomeKindV1,
        payload_digest: ContentDigest,
    ) -> Self {
        Self {
            tenant_id,
            process_id,
            sequence,
            outcome_id,
            causation_id,
            kind,
            payload_digest,
        }
    }
}

impl ProcessActionDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ProcessAction;

    /// Creates a stable independently idempotent action DTO.
    pub const fn new(
        tenant_id: TenantId,
        process_id: ProcessId,
        action_id: ActionId,
        step_id: StepId,
        attempt: u32,
        kind: ProcessActionKindV1,
        payload_digest: ContentDigest,
    ) -> Self {
        Self {
            tenant_id,
            process_id,
            action_id,
            step_id,
            attempt,
            kind,
            payload_digest,
        }
    }
}

impl CanonicalCommandDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::CanonicalCommand;

    /// Creates a typed canonical command DTO.
    pub const fn new(
        tenant_id: TenantId,
        action_id: ActionId,
        operation: OperationId,
        resource_ids: Vec<ResourceId>,
        payload_digest: ContentDigest,
    ) -> Self {
        Self {
            tenant_id,
            action_id,
            operation,
            resource_ids,
            payload_digest,
        }
    }
}

impl CanonicalEventDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::CanonicalEvent;
}

impl ManualReviewDtoV1 {
    /// Immutable schema identity for this DTO version.
    pub const SCHEMA: SchemaV1 = SchemaV1::ManualReview;

    /// Creates a durable manual-review escalation DTO.
    pub const fn new(
        tenant_id: TenantId,
        process_id: ProcessId,
        review_id: ReviewId,
        opened_at_sequence: u64,
        evidence_digest: ContentDigest,
    ) -> Self {
        Self {
            tenant_id,
            process_id,
            review_id,
            opened_at_sequence,
            evidence_digest,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
    fn deserialization_preserves_identifier_validation() {
        let error = serde_json::from_str::<ActionId>("\"not-an-action\"").unwrap_err();
        assert!(error.is_data());
    }
}
