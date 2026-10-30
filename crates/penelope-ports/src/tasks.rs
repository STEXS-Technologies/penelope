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

/// Immutable join between one persisted saga action and its recoverable task.
///
/// Construct from an authoritatively committed dispatch request. Construction
/// validates identity but does not prove that a process store committed it.
/// Private fields prevent callers from substituting one coordinate afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEffectBinding {
    request: EffectDispatchRequest,
    definition: TaskDefinition,
    scope: TaskScope,
}

impl TaskEffectBinding {
    /// Pins the full process scope, action, effect, step, attempt and task input.
    /// # Errors
    /// Rejects invalid requests and definitions.
    pub fn new(
        request: EffectDispatchRequest,
        definition: TaskDefinition,
    ) -> Result<Self, TaskError> {
        let scope = task_scope_for_effect(&request, definition)?;
        Ok(Self {
            request,
            definition,
            scope,
        })
    }

    /// Exact immutable dispatch, including its optional deadline.
    #[must_use]
    pub const fn request(&self) -> &EffectDispatchRequest {
        &self.request
    }

    /// Task identity remains stable across restarted generations.
    #[must_use]
    pub const fn scope(&self) -> TaskScope {
        self.scope
    }

    /// Pinned task code, schemas, effect contract and execution policy.
    #[must_use]
    pub const fn definition(&self) -> TaskDefinition {
        self.definition
    }

    /// Checks an independently reconstructed request against every pinned field.
    /// # Errors
    /// Rejects scope, action, effect, deadline, definition or input substitution.
    pub fn validate(
        &self,
        request: &EffectDispatchRequest,
        definition: TaskDefinition,
        input: &[u8],
    ) -> Result<(), TaskError> {
        if request != &self.request
            || definition != self.definition
            || task_scope_for_effect(request, definition)? != self.scope
            || input.len()
                > usize::try_from(definition.budget.max_input_bytes)
                    .map_err(|_size| TaskError::ScopeMismatch)?
            || Digest::of(input) != self.scope.input.digest()
        {
            return Err(TaskError::ScopeMismatch);
        }
        Ok(())
    }

    /// Stable result relay identity, independent of task generation and delivery
    /// leases. Use the same identity on every replay; still validate inbox payloads.
    #[must_use]
    pub fn relay_identity(&self) -> Digest {
        let mut bytes = b"penelope.task-effect-relay.v1\0".to_vec();
        bytes.extend_from_slice(&self.request.effect_key.canonical_bytes());
        bytes.extend_from_slice(&self.definition.fingerprint().digest().0);
        // The action identifier is last, so its variable length is unambiguous.
        bytes.extend_from_slice(self.request.action.action_id.as_str().as_bytes());
        Digest::of(&bytes)
    }
}

/// Translates authoritative task state into the existing effect port contract.
/// Unknown/exhausted external effects remain ambiguous. The caller must verify
/// output bytes and any application success condition before advancing a saga.
/// In particular, `Succeeded` here describes task execution only: effectful
/// completion requires independent effect evidence before claiming remote success.
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
