//! Versioned data-transfer objects for Penelope's public and adapter boundaries.
//!
//! These DTOs intentionally carry data only. They are not a workflow engine,
//! persistence model, transport client, or infrastructure implementation.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use serde::{Deserialize, Serialize};

/// A version-pinned immutable process definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDefinitionDtoV1 {
    /// Must be `penelope.process.definition.v1`.
    pub schema: String,
    /// Stable definition identity.
    pub definition_id: String,
    /// Immutable definition version selected when an instance starts.
    pub definition_version: String,
    /// Digest of the canonical definition representation.
    pub definition_digest: String,
    /// Ordered declared step identifiers.
    pub step_ids: Vec<String>,
}

/// A causally attributable process input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInputDtoV1 {
    /// Must be `penelope.process.input.v1`.
    pub schema: String,
    /// Isolated tenant scope.
    pub tenant_id: String,
    /// Target process instance.
    pub process_id: String,
    /// Immutable input identity used for inbox deduplication.
    pub input_id: String,
    /// Input category such as canonical event, timer firing, or review decision.
    pub kind: String,
    /// Canonical payload digest; raw payload storage is adapter policy.
    pub payload_digest: String,
}

/// One immutable fact in a process outcome log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutcomeDtoV1 {
    /// Must be `penelope.process.outcome.v1`.
    pub schema: String,
    /// Isolated tenant scope.
    pub tenant_id: String,
    /// Process instance that owns the outcome.
    pub process_id: String,
    /// Strictly ordered per-instance outcome sequence.
    pub sequence: u64,
    /// Immutable outcome identity.
    pub outcome_id: String,
    /// Causal input or action identity.
    pub causation_id: String,
    /// Outcome category.
    pub kind: String,
    /// Canonical payload digest.
    pub payload_digest: String,
}

/// A durable, independently idempotent process action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessActionDtoV1 {
    /// Must be `penelope.process.action.v1`.
    pub schema: String,
    /// Isolated tenant scope.
    pub tenant_id: String,
    /// Owning process instance.
    pub process_id: String,
    /// Stable action identity and external idempotency key.
    pub action_id: String,
    /// Pinned definition step identity.
    pub step_id: String,
    /// Attempt number for this action.
    pub attempt: u32,
    /// Action category: canonical command, timer, external effect, or review.
    pub kind: String,
    /// Canonical payload digest.
    pub payload_digest: String,
}

/// A canonical-state command submitted through an adapter port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalCommandDtoV1 {
    /// Must be `penelope.canonical.command.v1`.
    pub schema: String,
    /// Tenant authorized for the command.
    pub tenant_id: String,
    /// Penelope action ID reused as canonical idempotency ID.
    pub action_id: String,
    /// Declared canonical operation.
    pub operation: String,
    /// Expected resource scope identifiers.
    pub resource_ids: Vec<String>,
    /// Canonical command payload digest.
    pub payload_digest: String,
}

/// Evidence of a verified committed canonical-state result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalEventDtoV1 {
    /// Must be `penelope.canonical.event.v1`.
    pub schema: String,
    /// Tenant of the committed event.
    pub tenant_id: String,
    /// Immutable source delivery/event identity used by the inbox.
    pub source_event_id: String,
    /// Expected Penelope action/canonical idempotency identity.
    pub action_id: String,
    /// Canonical commit identity.
    pub commit_id: String,
    /// Canonical commit sequence.
    pub commit_sequence: u64,
    /// Committed canonical operation.
    pub operation: String,
    /// Affected resource scope identifiers.
    pub resource_ids: Vec<String>,
    /// Canonical event payload digest.
    pub payload_digest: String,
}

/// A durable request for authorized human resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualReviewDtoV1 {
    /// Must be `penelope.manual-review.v1`.
    pub schema: String,
    /// Isolated tenant scope.
    pub tenant_id: String,
    /// Owning process instance.
    pub process_id: String,
    /// Stable review identity.
    pub review_id: String,
    /// Outcome sequence at which review was opened.
    pub opened_at_sequence: u64,
    /// Redacted evidence digest.
    pub evidence_digest: String,
}
