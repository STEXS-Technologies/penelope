//! Hosted adapters can use async storage while embedded adapters implement the
//! synchronous `task::runtime` traits. Both obey the same atomic commit contract.

use crate::task::{BlobRef, TaskRecord, TaskScope, runtime::TaskPortError};
use crate::task::{
    Digest, EffectContract, TaskDefinition, TaskError, TaskId, TaskInputDigest, TaskProjection,
    TaskSequence, TaskStatus, TaskTenantId,
};
use crate::{EffectDispatchRequest, ExternalEffectEvidence, ExternalEffectState};
use async_trait::async_trait;

/// Binds a saga external effect to an independently recoverable task. Its
/// generation changes inside this scope; the saga attempt/effect key stays
/// pinned until task reconciliation resolves it. Existing saga types are intact.
/// # Errors
/// Rejects invalid effect requests and task definitions.
pub fn task_scope_for_effect(
    request: &EffectDispatchRequest,
    definition: TaskDefinition,
) -> Result<TaskScope, TaskError> {
    request
        .validate()
        .map_err(|_invalid| TaskError::ScopeMismatch)?;
    definition.validate()?;
    if request.action.kind != penelope_domain::ProcessActionKind::ExternalEffect {
        return Err(TaskError::ScopeMismatch);
    }
    Ok(TaskScope {
        tenant: TaskTenantId::from_digest(Digest::of(request.action.tenant_id.as_str().as_bytes())),
        task: TaskId::from_digest(Digest::of(&request.effect_key.canonical_bytes())),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest(request.action.payload_digest.0)),
    })
}

/// Translates authoritative task state into the existing effect port contract.
/// Unknown/exhausted external effects remain ambiguous. The caller must verify
/// output bytes and any application success condition before advancing a saga.
/// # Errors
/// Rejects a task projection bound to a different saga effect.
pub fn task_effect_evidence(
    request: &EffectDispatchRequest,
    projection: &TaskProjection,
) -> Result<ExternalEffectEvidence, TaskError> {
    if projection.scope() != task_scope_for_effect(request, projection.definition())? {
        return Err(TaskError::ScopeMismatch);
    }
    let state = match projection.status() {
        TaskStatus::Completed => ExternalEffectState::Succeeded,
        TaskStatus::Exhausted if projection.definition().effects == EffectContract::Pure => {
            ExternalEffectState::KnownFailure
        }
        TaskStatus::Empty
        | TaskStatus::Pending
        | TaskStatus::Running
        | TaskStatus::Faulted
        | TaskStatus::Reconciling
        | TaskStatus::Reconciled
        | TaskStatus::Ready
        | TaskStatus::Exhausted => ExternalEffectState::Unknown,
    };
    Ok(ExternalEffectEvidence {
        action_id: request.action.action_id.clone(),
        effect_key: request.effect_key.clone(),
        external_reference_id: None,
        state,
    })
}

/// Hosted journal counterpart; no runtime or storage implementation is required.
#[async_trait]
pub trait AsyncTaskJournal: Send + Sync {
    /// Reads one exact scoped record at the supplied sequence.
    async fn read(
        &self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError>;
    /// Atomically commits a CAS-checked record and optional immutable bytes.
    async fn commit(
        &self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError>;
    /// Reads exact bytes into caller-owned bounded memory without truncation.
    async fn read_blob(
        &self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError>;
}
