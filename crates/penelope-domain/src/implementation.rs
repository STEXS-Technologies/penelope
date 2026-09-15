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
use sha2::{Digest as _, Sha256};
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
    /// A migration identifier did not have its required canonical form.
    #[error("invalid definition migration identifier")]
    InvalidMigrationId,
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
    /// An outcome does not belong to the requested process scope.
    #[error("process outcome scope does not match the requested process")]
    OutcomeScopeMismatch,
}

/// Result of comparing a candidate definition with an already pinned one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefinitionCompatibility {
    /// The candidate is byte-for-byte equivalent under the pinned identity.
    Exact,
    /// The candidate keeps the definition identity but changes semantics and
    /// therefore requires an explicit migration before it can run.
    RequiresMigration,
    /// The candidate cannot be used for the pinned process at all.
    Incompatible,
}

/// Typed failure when a definition is not safe to use for a pinned process.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionCompatibilityError {
    /// Definition identity or version changed.
    #[error("definition identity or version is incompatible with the pinned process")]
    IdentityMismatch,
    /// Definition content changed without an explicit migration.
    #[error("definition content changed and requires an explicit migration")]
    RequiresMigration,
    /// The stored digest does not match the canonical v1 definition bytes.
    #[error("definition digest does not match canonical v1 encoding")]
    DigestMismatch,
}

/// Typed failure for registering a definition migration.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionMigrationError {
    /// A migration cannot change the immutable definition identity.
    #[error("definition migration changes the immutable definition identity")]
    IdentityMismatch,
    /// A migration must change version or semantics.
    #[error("definition migration is a no-op")]
    NoOp,
    /// The destination definition failed its own bound validation.
    #[error("definition migration destination is invalid")]
    InvalidDestination,
    /// The pinned source definition failed bound validation.
    #[error("definition migration source is invalid")]
    InvalidSource,
}

/// Typed failure for canonical wire encoding.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalEncodingError {
    /// The serializer could not encode the value.
    #[error("canonical wire encoding failed")]
    Serialization,
}

/// Canonical, deterministic bytes for a public value.
pub trait CanonicalWireBytes {
    /// Encodes the value with stable struct field order.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalEncodingError::Serialization`] if encoding fails.
    fn canonical_wire_bytes(&self) -> Result<Vec<u8>, CanonicalEncodingError>;
}

fn validate_identifier(prefix: &str, value: &str) -> bool {
    value.starts_with(prefix)
        && value.len() > prefix.len()
        && value.len() <= MAX_IDENTIFIER_LENGTH
        && value[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn encode_identifier(bytes: &mut Vec<u8>, value: &str) {
    let length = u16::try_from(value.len()).unwrap_or(u16::MAX);
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
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
identifier!(
    MigrationId,
    "mig_",
    InvalidMigrationId,
    "Explicit definition migration identity."
);

/// Fixed-size digest of canonical payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentDigest(pub [u8; 32]);

impl ContentDigest {
    /// Computes the stable SHA-256 digest used by Penelope's v1 canonical
    /// encoding contract.
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut value = [0_u8; 32];
        value.copy_from_slice(&digest);
        Self(value)
    }
}

/// A deterministic millisecond timestamp supplied by an injected clock.
///
/// The pure engine must never read wall clock time directly; it receives this
/// typed value through an application command or port response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LogicalTime(pub u64);

/// Typed process input category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessInputKind {
    /// A verified committed canonical event.
    CanonicalEvent,
    /// A durable timer firing.
    TimerFired,
    /// An authorized manual-review decision.
    ManualResolution,
}

/// Typed append-only process outcome category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessOutcomeKind {
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
pub enum ProcessActionKind {
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
pub enum CausationId {
    /// The outcome was caused by an accepted input.
    Input(InputId),
    /// The outcome was caused by an action.
    Action(ActionId),
}

/// Attributable source that recorded one immutable process outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutcomeActor {
    /// The deterministic process runtime recorded the outcome.
    System,
    /// An authenticated principal recorded an authorized outcome.
    Principal(PrincipalId),
}

/// Attributable immutable facts recorded in one process outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutcomeFact {
    /// Immutable idempotency identity for this outcome.
    pub outcome_id: OutcomeId,
    /// Typed causal input or action identity.
    pub causation_id: CausationId,
    /// Attributable source that recorded this immutable fact.
    pub actor: OutcomeActor,
    /// Deterministic recorded-at time supplied by an injected clock.
    pub occurred_at: LogicalTime,
    /// Typed outcome category.
    pub kind: ProcessOutcomeKind,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

impl ProcessOutcomeFact {
    /// Groups the immutable fact fields for a process outcome record.
    pub const fn new(
        outcome_id: OutcomeId,
        causation_id: CausationId,
        actor: OutcomeActor,
        occurred_at: LogicalTime,
        kind: ProcessOutcomeKind,
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
pub struct ProcessDefinition {
    /// Stable definition identity.
    pub definition_id: DefinitionId,
    /// Immutable definition version selected when an instance starts.
    pub definition_version: DefinitionVersion,
    /// Digest of the canonical definition representation.
    pub definition_digest: ContentDigest,
    /// Ordered declared step identifiers.
    pub step_ids: Vec<StepId>,
}

/// Explicit, immutable registration record for a definition migration.
///
/// The executor never applies this record implicitly. A composition root must
/// validate and authorize the migration, transform any process state under its
/// own policy, and retain this record alongside the new definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionMigration {
    /// Stable migration identity used for idempotent registration.
    pub migration_id: MigrationId,
    /// Definition identity shared by both versions.
    pub definition_id: DefinitionId,
    /// Previously pinned version.
    pub from_version: DefinitionVersion,
    /// Previously registered semantic digest.
    pub from_digest: ContentDigest,
    /// Replacement version.
    pub to_version: DefinitionVersion,
    /// Replacement semantic digest.
    pub to_digest: ContentDigest,
}

impl DefinitionMigration {
    /// Creates and validates an explicit definition migration record.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a no-op migration or an identity change.
    pub fn new(
        migration_id: MigrationId,
        definition_id: DefinitionId,
        from_version: DefinitionVersion,
        from_digest: ContentDigest,
        to_version: DefinitionVersion,
        to_digest: ContentDigest,
    ) -> Result<Self, DefinitionMigrationError> {
        let migration = Self {
            migration_id,
            definition_id,
            from_version,
            from_digest,
            to_version,
            to_digest,
        };
        migration.validate()?;
        Ok(migration)
    }

    /// Validates that the record represents an actual same-definition change.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a no-op migration.
    pub fn validate(&self) -> Result<(), DefinitionMigrationError> {
        if self.from_version == self.to_version && self.from_digest == self.to_digest {
            return Err(DefinitionMigrationError::NoOp);
        }
        Ok(())
    }

    /// Requires a candidate definition to match the migration destination.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionMigrationError::IdentityMismatch`] when the
    /// candidate uses another definition identity or destination version/digest.
    pub fn validate_destination(
        &self,
        candidate: &ProcessDefinition,
    ) -> Result<(), DefinitionMigrationError> {
        if candidate.validate().is_err() {
            return Err(DefinitionMigrationError::InvalidDestination);
        }
        if candidate.definition_id != self.definition_id
            || candidate.definition_version != self.to_version
            || candidate.definition_digest != self.to_digest
        {
            return Err(DefinitionMigrationError::IdentityMismatch);
        }
        Ok(())
    }

    /// Validates both ends of a migration against the pinned source and
    /// replacement definition before a running process may switch semantics.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the source identity/digest or destination
    /// identity/version/digest does not match this migration record.
    pub fn validate_source_and_destination(
        &self,
        source: &ProcessDefinition,
        destination: &ProcessDefinition,
    ) -> Result<(), DefinitionMigrationError> {
        if source.validate().is_err() {
            return Err(DefinitionMigrationError::InvalidSource);
        }
        if source.definition_id != self.definition_id
            || source.definition_version != self.from_version
            || source.definition_digest != self.from_digest
        {
            return Err(DefinitionMigrationError::IdentityMismatch);
        }
        self.validate_destination(destination)
    }
}

/// A causally attributable process input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInput {
    /// Isolated tenant scope.
    pub tenant_id: TenantId,
    /// Target process instance.
    pub process_id: ProcessId,
    /// Immutable input identity used for inbox deduplication.
    pub input_id: InputId,
    /// Typed input category.
    pub kind: ProcessInputKind,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

/// Explicitly versioned public envelope for one process input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInputEnvelope {
    /// Typed process input supplied to the application boundary.
    pub input: ProcessInput,
}

impl ProcessInputEnvelope {
    pub const fn new(input: ProcessInput) -> Self {
        Self { input }
    }

    /// Validates the enclosed process input.
    ///
    /// # Errors
    ///
    /// Returns the typed input validation error when any identifier is invalid.
    pub const fn validate(&self) -> Result<(), DomainError> {
        self.input.validate()
    }
}

/// One immutable fact in a process outcome log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutcome {
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
    pub causation_id: CausationId,
    /// Attributable source that recorded this immutable fact.
    pub actor: OutcomeActor,
    /// Deterministic recorded-at time supplied by an injected clock.
    pub occurred_at: LogicalTime,
    /// Typed outcome category.
    pub kind: ProcessOutcomeKind,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

/// Immutable identity and definition scope of one process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessScope {
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

impl ProcessScope {
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

    /// Encodes the complete pinned scope into an unambiguous, deterministic
    /// byte sequence suitable as input to a consumer-selected cryptographic
    /// digest. Length-prefixing prevents concatenation ambiguity; callers must
    /// hash these bytes with a documented algorithm rather than hash display
    /// text or serialized JSON.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode_identifier(&mut bytes, self.tenant_id.as_str());
        encode_identifier(&mut bytes, self.process_id.as_str());
        encode_identifier(&mut bytes, self.definition_id.as_str());
        encode_identifier(&mut bytes, self.definition_version.as_str());
        bytes.extend_from_slice(&self.definition_digest.0);
        bytes
    }
}

/// A durable, independently idempotent process action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessAction {
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
    pub kind: ProcessActionKind,
    /// Canonical payload digest.
    pub payload_digest: ContentDigest,
}

/// Stable semantic idempotency key for one process effect attempt.
///
/// Unlike [`ActionId`], this key is derived from the pinned process definition
/// and the effect's semantic coordinates. Adapters can use it to detect a
/// duplicate semantic dispatch without reconstructing an untyped string key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectKey {
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
    pub kind: ProcessActionKind,
    /// Canonical payload semantics for this effect attempt.
    pub payload_digest: ContentDigest,
}

impl EffectKey {
    /// Encodes every semantic effect coordinate deterministically.
    ///
    /// The encoding is length-prefixed and includes the action kind and
    /// attempt, so two distinct effects cannot collide by concatenation.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode_identifier(&mut bytes, self.tenant_id.as_str());
        encode_identifier(&mut bytes, self.process_id.as_str());
        encode_identifier(&mut bytes, self.definition_id.as_str());
        encode_identifier(&mut bytes, self.definition_version.as_str());
        bytes.extend_from_slice(&self.definition_digest.0);
        encode_identifier(&mut bytes, self.step_id.as_str());
        bytes.extend_from_slice(&self.attempt.to_be_bytes());
        bytes.push(match self.kind {
            ProcessActionKind::CanonicalCommand => 0,
            ProcessActionKind::Timer => 1,
            ProcessActionKind::ExternalEffect => 2,
            ProcessActionKind::ManualReview => 3,
        });
        bytes.extend_from_slice(&self.payload_digest.0);
        bytes
    }
}

/// A canonical-state command submitted through an adapter port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalCommand {
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
pub struct CanonicalEvent {
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
pub struct ManualReview {
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
    pub expires_at: Option<LogicalTime>,
    /// Redacted evidence digest.
    pub evidence_digest: ContentDigest,
}

fn canonical_serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalEncodingError> {
    serde_json::to_vec(value).map_err(|_serialization_error| CanonicalEncodingError::Serialization)
}

macro_rules! canonical_wire_impl {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl CanonicalWireBytes for $ty {
                fn canonical_wire_bytes(&self) -> Result<Vec<u8>, CanonicalEncodingError> {
                    canonical_serialize(self)
                }
            }
        )+
    };
}

canonical_wire_impl!(
    ContentDigest,
    LogicalTime,
    ProcessInputKind,
    ProcessOutcomeKind,
    ProcessActionKind,
    CausationId,
    OutcomeActor,
    ProcessOutcomeFact,
    ProcessDefinition,
    DefinitionMigration,
    ProcessInput,
    ProcessInputEnvelope,
    ProcessOutcome,
    ProcessScope,
    ProcessAction,
    EffectKey,
    CanonicalCommand,
    CanonicalEvent,
    ManualReview,
);

impl ProcessDefinition {
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

    /// Encodes definition identity and semantics without including the stored
    /// digest, avoiding a self-referential hash input.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode_identifier(&mut bytes, self.definition_id.as_str());
        encode_identifier(&mut bytes, self.definition_version.as_str());
        let step_count = u32::try_from(self.step_ids.len()).unwrap_or(u32::MAX);
        bytes.extend_from_slice(&step_count.to_be_bytes());
        for step_id in &self.step_ids {
            encode_identifier(&mut bytes, step_id.as_str());
        }
        bytes
    }

    /// Computes the canonical SHA-256 definition digest for registration.
    #[must_use]
    pub fn computed_digest(&self) -> ContentDigest {
        ContentDigest::sha256(&self.canonical_bytes())
    }

    /// Requires the stored digest to match the canonical v1 definition bytes.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionCompatibilityError::DigestMismatch`] when the
    /// supplied digest is not reproducible from the definition contents.
    pub fn require_canonical_digest(&self) -> Result<(), DefinitionCompatibilityError> {
        if self.definition_digest == self.computed_digest() {
            Ok(())
        } else {
            Err(DefinitionCompatibilityError::DigestMismatch)
        }
    }

    /// Classifies whether this definition can replace a pinned definition.
    #[must_use]
    pub fn compatibility_with(&self, pinned: &Self) -> DefinitionCompatibility {
        if self.definition_id != pinned.definition_id
            || self.definition_version != pinned.definition_version
        {
            DefinitionCompatibility::Incompatible
        } else if self.definition_digest != pinned.definition_digest
            || self.step_ids != pinned.step_ids
        {
            DefinitionCompatibility::RequiresMigration
        } else {
            DefinitionCompatibility::Exact
        }
    }

    /// Fails closed unless this definition is exactly the pinned definition.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionCompatibilityError::IdentityMismatch`] when the
    /// definition identity/version differs, or
    /// [`DefinitionCompatibilityError::RequiresMigration`] when semantics
    /// changed under the same identity.
    pub fn require_exact_compatibility(
        &self,
        pinned: &Self,
    ) -> Result<(), DefinitionCompatibilityError> {
        match self.compatibility_with(pinned) {
            DefinitionCompatibility::Exact => Ok(()),
            DefinitionCompatibility::RequiresMigration => {
                Err(DefinitionCompatibilityError::RequiresMigration)
            }
            DefinitionCompatibility::Incompatible => {
                Err(DefinitionCompatibilityError::IdentityMismatch)
            }
        }
    }
}

impl ProcessInput {
    /// Creates a typed process input DTO.
    pub const fn new(
        tenant_id: TenantId,
        process_id: ProcessId,
        input_id: InputId,
        kind: ProcessInputKind,
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

    /// Validates the immutable input typed invariant.
    ///
    /// # Errors
    ///
    /// Returns a typed error when this record declares another DTO schema.
    pub const fn validate(&self) -> Result<(), DomainError> {
        Ok(())
    }
}

impl ProcessOutcome {
    /// Creates an immutable process outcome DTO.
    pub fn new(scope: ProcessScope, sequence: u64, fact: ProcessOutcomeFact) -> Self {
        Self {
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

    /// Validates the immutable outcome record typed invariant.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the record declares a different DTO schema.
    pub const fn validate(&self) -> Result<(), DomainError> {
        Ok(())
    }

    /// Returns the immutable process scope carried by this outcome.
    #[must_use]
    pub fn scope(&self) -> ProcessScope {
        ProcessScope::new(
            self.tenant_id.clone(),
            self.process_id.clone(),
            self.definition_id.clone(),
            self.definition_version.clone(),
            self.definition_digest,
        )
    }

    /// Validates both the DTO schema and its exact pinned process scope.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the schema or scope is invalid.
    pub fn validate_for_scope(&self, expected: &ProcessScope) -> Result<(), DomainError> {
        self.validate()?;
        if self.scope() == *expected {
            Ok(())
        } else {
            Err(DomainError::OutcomeScopeMismatch)
        }
    }
}

impl ProcessAction {
    /// Creates a stable independently idempotent action DTO.
    pub fn new(
        scope: ProcessScope,
        action_id: ActionId,
        step_id: StepId,
        attempt: u32,
        kind: ProcessActionKind,
        payload_digest: ContentDigest,
    ) -> Self {
        Self {
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

    /// Validates the immutable action typed invariant.
    ///
    /// # Errors
    ///
    /// Returns a typed error when this record declares another DTO schema.
    pub const fn validate(&self) -> Result<(), DomainError> {
        Ok(())
    }

    /// Returns the immutable process and definition scope for this action.
    #[must_use]
    pub fn scope(&self) -> ProcessScope {
        ProcessScope::new(
            self.tenant_id.clone(),
            self.process_id.clone(),
            self.definition_id.clone(),
            self.definition_version.clone(),
            self.definition_digest,
        )
    }

    /// Derives the stable semantic idempotency key for this action attempt.
    pub fn effect_key(&self) -> EffectKey {
        EffectKey {
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

impl CanonicalCommand {
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
        validate_canonical_resource_scope(&self.resource_ids)
    }
}

impl CanonicalEvent {
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

impl ManualReview {
    /// Creates a durable manual-review escalation DTO.
    pub fn new(
        scope: ProcessScope,
        review_id: ReviewId,
        opened_at_sequence: u64,
        expires_at: Option<LogicalTime>,
        evidence_digest: ContentDigest,
    ) -> Self {
        Self {
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

    /// Validates the immutable manual-review typed invariant.
    ///
    /// # Errors
    ///
    /// Returns a typed error when this record declares another DTO schema.
    pub const fn validate(&self) -> Result<(), DomainError> {
        Ok(())
    }

    /// Returns the immutable process and definition scope for this review.
    #[must_use]
    pub fn scope(&self) -> ProcessScope {
        ProcessScope::new(
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
        let scope = ProcessScope::new(
            id::<TenantId>("tnt_game"),
            id::<ProcessId>("prc_trade"),
            id::<DefinitionId>("def_trade"),
            id::<DefinitionVersion>("dfv_one"),
            ContentDigest([1; 32]),
        );
        let action = ProcessAction::new(
            scope.clone(),
            id::<ActionId>("act_first"),
            id::<StepId>("stp_settle"),
            0,
            ProcessActionKind::CanonicalCommand,
            ContentDigest([2; 32]),
        );
        let same_semantics_new_action_id = ProcessAction::new(
            scope,
            id::<ActionId>("act_second"),
            id::<StepId>("stp_settle"),
            0,
            ProcessActionKind::CanonicalCommand,
            ContentDigest([2; 32]),
        );
        let retry = ProcessAction::new(
            ProcessScope::new(
                id::<TenantId>("tnt_game"),
                id::<ProcessId>("prc_trade"),
                id::<DefinitionId>("def_trade"),
                id::<DefinitionVersion>("dfv_one"),
                ContentDigest([1; 32]),
            ),
            id::<ActionId>("act_retry"),
            id::<StepId>("stp_settle"),
            1,
            ProcessActionKind::CanonicalCommand,
            ContentDigest([3; 32]),
        );
        let changed_payload = ProcessAction::new(
            ProcessScope::new(
                id::<TenantId>("tnt_game"),
                id::<ProcessId>("prc_trade"),
                id::<DefinitionId>("def_trade"),
                id::<DefinitionVersion>("dfv_one"),
                ContentDigest([1; 32]),
            ),
            id::<ActionId>("act_changed_payload"),
            id::<StepId>("stp_settle"),
            0,
            ProcessActionKind::CanonicalCommand,
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
    fn canonical_bytes_are_deterministic_and_bind_effect_semantics() {
        let scope = ProcessScope::new(
            id("tnt_game"),
            id("prc_trade"),
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([1; 32]),
        );
        let action = ProcessAction::new(
            scope.clone(),
            id("act_first"),
            id("stp_settle"),
            0,
            ProcessActionKind::CanonicalCommand,
            ContentDigest([2; 32]),
        );
        let same_scope = scope.clone();
        assert_eq!(scope.canonical_bytes(), same_scope.canonical_bytes());
        let first = action.effect_key().canonical_bytes();
        let mut retry = action;
        retry.attempt = 1;
        assert_ne!(first, retry.effect_key().canonical_bytes());
    }

    #[test]
    fn deserialization_preserves_identifier_validation() {
        let error = serde_json::from_str::<ActionId>("\"not-an-action\"").unwrap_err();
        assert!(error.is_data());
    }

    #[test]
    fn outcome_scope_validation_rejects_cross_process_records() {
        let scope = ProcessScope::new(
            id("tnt_market"),
            id("prc_trade"),
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([2; 32]),
        );
        let outcome = ProcessOutcome::new(
            scope.clone(),
            0,
            ProcessOutcomeFact::new(
                id("out_started"),
                CausationId::Input(id("inp_event")),
                OutcomeActor::System,
                LogicalTime(0),
                ProcessOutcomeKind::Started,
                ContentDigest([3; 32]),
            ),
        );
        let other = ProcessScope::new(
            id("tnt_market"),
            id("prc_other"),
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([2; 32]),
        );
        assert_eq!(
            outcome.validate_for_scope(&other),
            Err(DomainError::OutcomeScopeMismatch)
        );
        assert!(outcome.validate_for_scope(&scope).is_ok());
    }

    #[test]
    fn definition_validation_bounds_and_deduplicates_steps() {
        let oversized = ProcessDefinition::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([0; 32]),
            vec![id::<StepId>("stp_lock"); MAX_DEFINITION_STEPS.saturating_add(1)],
        );
        assert_eq!(
            oversized.unwrap_err(),
            DomainError::DefinitionStepLimitExceeded
        );

        let duplicate = ProcessDefinition::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([0; 32]),
            vec![id("stp_lock"), id("stp_lock")],
        );
        assert_eq!(duplicate.unwrap_err(), DomainError::DuplicateStepId);
    }

    #[test]
    fn definition_compatibility_fails_closed_on_identity_and_semantic_changes() {
        let pinned = ProcessDefinition::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([1; 32]),
            vec![id("stp_lock"), id("stp_settle")],
        )
        .unwrap();
        assert_eq!(
            pinned.compatibility_with(&pinned),
            DefinitionCompatibility::Exact
        );

        let changed_content = ProcessDefinition::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([2; 32]),
            vec![id("stp_lock"), id("stp_settle")],
        )
        .unwrap();
        assert_eq!(
            changed_content.compatibility_with(&pinned),
            DefinitionCompatibility::RequiresMigration
        );
        assert_eq!(
            changed_content.require_exact_compatibility(&pinned),
            Err(DefinitionCompatibilityError::RequiresMigration)
        );

        let changed_identity = ProcessDefinition::new(
            id("def_other"),
            id("dfv_one"),
            ContentDigest([1; 32]),
            vec![id("stp_lock"), id("stp_settle")],
        )
        .unwrap();
        assert_eq!(
            changed_identity.compatibility_with(&pinned),
            DefinitionCompatibility::Incompatible
        );
        assert_eq!(
            changed_identity.require_exact_compatibility(&pinned),
            Err(DefinitionCompatibilityError::IdentityMismatch)
        );
    }

    #[test]
    fn canonical_definition_digest_is_reproducible_and_rejects_tampering() {
        let mut definition = ProcessDefinition::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([0; 32]),
            vec![id("stp_lock"), id("stp_settle")],
        )
        .unwrap();
        definition.definition_digest = definition.computed_digest();
        assert_eq!(definition.require_canonical_digest(), Ok(()));
        let encoded = definition.canonical_bytes();
        assert_eq!(
            ContentDigest::sha256(&encoded),
            definition.definition_digest
        );
        definition.step_ids.reverse();
        assert_eq!(
            definition.require_canonical_digest(),
            Err(DefinitionCompatibilityError::DigestMismatch)
        );
    }

    #[test]
    fn definition_migration_binds_identity_and_destination_digest() {
        let migration = DefinitionMigration::new(
            id("mig_trade_v2"),
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([1; 32]),
            id("dfv_two"),
            ContentDigest([2; 32]),
        )
        .unwrap();
        let destination = ProcessDefinition::new(
            id("def_trade"),
            id("dfv_two"),
            ContentDigest([2; 32]),
            vec![id("stp_lock")],
        )
        .unwrap();
        let source = ProcessDefinition::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([1; 32]),
            vec![id("stp_lock")],
        )
        .unwrap();
        assert_eq!(
            migration.validate_source_and_destination(&source, &destination),
            Ok(())
        );
        let wrong_source = ProcessDefinition::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([9; 32]),
            vec![id("stp_lock")],
        )
        .unwrap();
        assert_eq!(
            migration.validate_source_and_destination(&wrong_source, &destination),
            Err(DefinitionMigrationError::IdentityMismatch)
        );
        assert_eq!(migration.validate_destination(&destination), Ok(()));
        let wrong = ProcessDefinition::new(
            id("def_other"),
            id("dfv_two"),
            ContentDigest([2; 32]),
            vec![id("stp_lock")],
        )
        .unwrap();
        assert_eq!(
            migration.validate_destination(&wrong),
            Err(DefinitionMigrationError::IdentityMismatch)
        );
        let mut malformed = destination;
        malformed.step_ids.clear();
        assert_eq!(
            migration.validate_destination(&malformed),
            Err(DefinitionMigrationError::InvalidDestination)
        );
        assert_eq!(
            DefinitionMigration::new(
                id("mig_noop"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([1; 32]),
                id("dfv_one"),
                ContentDigest([1; 32]),
            ),
            Err(DefinitionMigrationError::NoOp)
        );
    }

    #[test]
    fn every_public_wire_value_has_deterministic_canonical_bytes() {
        let input = ProcessInput::new(
            id("tnt_game"),
            id("prc_trade"),
            id("inp_start"),
            ProcessInputKind::CanonicalEvent,
            ContentDigest([4; 32]),
        );
        let envelope = ProcessInputEnvelope::new(input);
        let first = envelope.canonical_wire_bytes().unwrap();
        let second = envelope.canonical_wire_bytes().unwrap();
        assert_eq!(first, second);
        assert!(!first.is_empty());
    }

    #[test]
    fn canonical_scope_validation_bounds_and_deduplicates_resources() {
        let duplicate = CanonicalCommand::new(
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
