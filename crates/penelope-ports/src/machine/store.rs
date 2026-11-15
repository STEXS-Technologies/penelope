//! Host authentication and journal authority stay outside the pure machine.
use super::*;
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MachineStoreError {
    #[error("machine operation denied")]
    Denied,
    #[error("machine scope or pinned definition mismatch")]
    ScopeMismatch,
    #[error("machine predecessor is stale")]
    StaleHead,
    #[error("machine identity already has different immutable content")]
    Conflict,
    #[error("machine journal capacity exhausted")]
    Capacity,
    #[error("machine journal invariant or stored bytes are corrupt")]
    Corrupt,
    #[error("machine journal unavailable; commit outcome may require lookup")]
    Unavailable,
}

/// New domain-specific policy operations; legacy exhaustive enums are unchanged.
/// Context is supplied by the host, never inferred from payload tenant assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineOperation {
    Bootstrap {
        state_digest: [u8; 32],
    },
    Commit {
        expected: MachineRevision,
        event: MachineEventId,
        identity: MachineCommitId,
    },
    ReadHead,
    ClaimIntents {
        owner: MachineLeaseOwner,
    },
    ReadIntent {
        intent: MachineIntentId,
    },
    ValidateIntent {
        consumer: MachineSourceId,
        intent: MachineIntentId,
        owner: MachineLeaseOwner,
        token: MachineLeaseToken,
    },
    AcknowledgeIntent {
        intent: MachineIntentId,
        commit: MachineCommitId,
        owner: MachineLeaseOwner,
        token: MachineLeaseToken,
        consumer: MachineSourceId,
    },
    Lookup {
        event: MachineEventId,
    },
    ReadHistory {
        after: MachineRevision,
        through: MachineRevision,
    },
}

#[async_trait]
pub trait MachineAuthorizer: Send + Sync {
    type Context: Send + Sync;
    /// Fail closed before reading existence, payloads or returning duplicates.
    /// # Errors
    /// Denies unauthenticated, unauthorized or unavailable policy decisions.
    async fn authorize(
        &self,
        context: &Self::Context,
        scope: MachineScope,
        operation: MachineOperation,
    ) -> Result<(), MachineStoreError>;
}

/// Immutable genesis bytes prepared from invariant-checked application state.
/// Only preparation creates this request; restored snapshots cannot be admitted
/// as new genesis requests. Retain its Arc to retry an uncertain bootstrap ACK.
pub struct PreparedMachineBootstrap {
    scope: MachineScope,
    state: Vec<u8>,
    digest: [u8; 32],
}
impl PreparedMachineBootstrap {
    pub const fn scope(&self) -> MachineScope {
        self.scope
    }
    pub fn state(&self) -> &[u8] {
        &self.state
    }
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

/// Validates and encodes a genesis snapshot without executing a transition.
/// # Errors
/// Refuses wrong model/codec definition, invalid state, encoding or byte limits.
pub fn prepare_machine_bootstrap<M: Machine, C: MachineCodec<M>>(
    model: &M,
    codec: &C,
    scope: MachineScope,
    state: &M::State,
    scratch: &mut [u8],
    limits: MachinePayloadLimits,
) -> Result<PreparedMachineBootstrap, MachinePreparationError<M::Error, C::Error>> {
    if model.definition() != scope.definition || codec.definition() != scope.definition {
        return Err(MachineFailure::Kernel(MachineError::DefinitionMismatch).into());
    }
    model
        .validate(state)
        .map_err(|error| MachinePreparationError::Machine(MachineFailure::Model(error)))?;
    let output = scratch
        .get_mut(..limits.max_state_bytes)
        .ok_or(MachinePreparationError::Capacity)?;
    let len = codec
        .encode_state(state, output)
        .map_err(MachinePreparationError::Codec)?;
    let bytes = output
        .get(..len)
        .filter(|bytes| bytes.len() <= limits.max_batch_bytes)
        .ok_or(MachinePreparationError::Capacity)?;
    Ok(PreparedMachineBootstrap {
        scope,
        digest: Digest::of(bytes).0,
        state: copy_bytes(bytes).ok_or(MachinePreparationError::Capacity)?,
    })
}

/// Checked coherent head snapshot. A trusted store must additionally verify the
/// last receipt against its retained commit; a hash does not authenticate storage.
pub struct MachineSnapshot {
    head: MachineHead,
    state: Vec<u8>,
    digest: [u8; 32],
    last: Option<MachineCommitReceipt>,
}
impl MachineSnapshot {
    /// # Errors
    /// Refuses oversized/mismatched bytes or an incoherent head/receipt pair.
    pub fn decode(
        head: MachineHead,
        state: &[u8],
        digest: [u8; 32],
        last: Option<MachineCommitReceipt>,
        limits: MachinePayloadLimits,
    ) -> Result<Self, MachineStoreError> {
        if state.len() > limits.max_state_bytes || state.len() > limits.max_batch_bytes {
            return Err(MachineStoreError::Capacity);
        }
        if Digest::of(state).0 != digest
            || last.map_or_else(
                || head.revision != MachineRevision::ZERO,
                |receipt| receipt.head != head || head.revision == MachineRevision::ZERO,
            )
        {
            return Err(MachineStoreError::Corrupt);
        }
        Ok(Self {
            head,
            state: copy_bytes(state).ok_or(MachineStoreError::Capacity)?,
            digest,
            last,
        })
    }
    pub const fn head(&self) -> MachineHead {
        self.head
    }
    pub fn state(&self) -> &[u8] {
        &self.state
    }
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub const fn last(&self) -> Option<MachineCommitReceipt> {
        self.last
    }
    /// Restores state only; durable pending intents retain journal authority.
    /// # Errors
    /// Refuses scope, codec, model, canonical-state or capacity violations.
    pub fn restore<M: Machine, C: MachineCodec<M>, const INTENTS: usize>(
        &self,
        model: M,
        codec: &C,
        scope: MachineScope,
        scratch: &mut [u8],
        limits: IntentLimits,
    ) -> Result<MemoryMachine<M, INTENTS>, MachinePreparationError<M::Error, C::Error>> {
        restore_snapshot(model, codec, scope, self.head, &self.state, scratch, limits)
    }
}

pub(super) fn restore_snapshot<M: Machine, C: MachineCodec<M>, const INTENTS: usize>(
    model: M,
    codec: &C,
    scope: MachineScope,
    head: MachineHead,
    bytes: &[u8],
    scratch: &mut [u8],
    limits: IntentLimits,
) -> Result<MemoryMachine<M, INTENTS>, MachinePreparationError<M::Error, C::Error>> {
    if head.scope != scope {
        return Err(MachineFailure::Kernel(MachineError::ScopeMismatch).into());
    }
    if model.definition() != scope.definition || codec.definition() != scope.definition {
        return Err(MachineFailure::Kernel(MachineError::DefinitionMismatch).into());
    }
    let state = codec
        .decode_state(bytes)
        .map_err(MachinePreparationError::Codec)?;
    let len = codec
        .encode_state(&state, scratch)
        .map_err(MachinePreparationError::Codec)?;
    if scratch.get(..len) != Some(bytes) {
        return Err(MachinePreparationError::PayloadMismatch);
    }
    MemoryMachine::restore(model, head, state, limits).map_err(Into::into)
}

/// Stable range under an explicitly captured upper revision. Revisions are native
/// machine coordinates; no offsets, clocks or latest-head substitutions occur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineHistoryRequest {
    pub scope: MachineScope,
    pub after: MachineRevision,
    pub through: MachineRevision,
    pub max_records: u32,
    pub max_bytes: usize,
}
pub struct MachineHistoryPage<const INTENTS: usize> {
    pub records: Vec<MachineStoredCommit<INTENTS>>,
    /// Last returned revision when further records remain in the requested range.
    pub next: Option<MachineRevision>,
}

/// Every method authorizes the host context before any scoped information leaks.
/// Implementations bind one tenant/instance to one pinned definition, enforce
/// quotas and refuse malformed persisted data. Commit is a single local
/// transaction binding event, predecessor CAS, state and all ordered intents.
#[async_trait]
pub trait MachineStore<const INTENTS: usize>: Send + Sync {
    type Context: Send + Sync;
    /// Exact repeated genesis is idempotent, including after later commits; a
    /// different genesis/definition conflicts and never resets an existing head.
    async fn bootstrap(
        &self,
        context: &Self::Context,
        request: Arc<PreparedMachineBootstrap>,
    ) -> Result<(), MachineStoreError>;
    /// Exact-event duplicate lookup precedes predecessor CAS. Conflicting content
    /// never succeeds; a lost ACK must be resolved through authoritative lookup.
    async fn commit(
        &self,
        context: &Self::Context,
        batch: Arc<MachineCommitBatch<INTENTS>>,
    ) -> Result<MachineCommitReceipt, MachineStoreError>;
    /// Reads state/head/last commit in one coherent snapshot.
    async fn load_head(
        &self,
        context: &Self::Context,
        scope: MachineScope,
    ) -> Result<Option<MachineSnapshot>, MachineStoreError>;
    async fn lookup(
        &self,
        context: &Self::Context,
        scope: MachineScope,
        event: MachineEventId,
    ) -> Result<Option<MachineStoredCommit<INTENTS>>, MachineStoreError>;
    /// Bounded pagination never silently skips an oversized or missing record.
    async fn history(
        &self,
        context: &Self::Context,
        request: MachineHistoryRequest,
    ) -> Result<MachineHistoryPage<INTENTS>, MachineStoreError>;
}
