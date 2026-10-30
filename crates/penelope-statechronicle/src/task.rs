//! Optional checkpoint-state verification boundary. Penelope's journal remains
//! independent. Cryptographic proof verification belongs in the implementing
//! StateChronicle adapter, before it returns this scoped evidence.
use penelope_ports::task::{
    BlobRef, Digest, TaskExecutionEvent, TaskGeneration, TaskRecord, TaskScope,
};
use thiserror::Error;

/// Independently verified state evidence. Digests alone are not proof; adapters
/// must verify authenticity, authoritative version and exact checkpoint binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointStateEvidence {
    pub scope: TaskScope,
    pub generation: TaskGeneration,
    pub checkpoint: BlobRef,
    pub state_version: u64,
    pub state_digest: Digest,
    pub proof_digest: Digest,
}

/// Optional application-supplied verifier; no client, storage or runtime coupling.
pub trait CheckpointStateVerifier {
    /// Independently verifies the checkpoint's binding to authoritative state.
    /// # Errors
    /// Returns verification failure or unavailable integrity service.
    fn verify(&self, record: &TaskRecord) -> Result<CheckpointStateEvidence, CheckpointStateError>;
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointStateError {
    #[error("record is not a task checkpoint commit")]
    NotCheckpoint,
    #[error("verified checkpoint state has a different scope, generation or blob")]
    BindingMismatch,
    #[error("independent state verification failed")]
    VerificationFailed,
}

const fn checkpoint(record: &TaskRecord) -> Result<BlobRef, CheckpointStateError> {
    match record.event {
        TaskExecutionEvent::CheckpointCommitted { checkpoint }
        | TaskExecutionEvent::CheckpointMigrated { checkpoint, .. } => Ok(checkpoint),
        TaskExecutionEvent::IntentCommitted { .. }
        | TaskExecutionEvent::Started
        | TaskExecutionEvent::FaultObserved { .. }
        | TaskExecutionEvent::ReconciliationRequired
        | TaskExecutionEvent::Reconciled { .. }
        | TaskExecutionEvent::RecoveryPlanned { .. }
        | TaskExecutionEvent::Restarted
        | TaskExecutionEvent::Completed { .. }
        | TaskExecutionEvent::RecoveryExhausted => Err(CheckpointStateError::NotCheckpoint),
    }
}

/// Invokes the trusted verifier and binds its evidence to the exact checkpoint.
///
/// Use this combined path when checkpoint acceptance requires independent state
/// verification. The application-supplied verifier owns proof authenticity, key
/// policy and authoritative state version checks; this function does not turn a
/// digest into proof. A failed verifier never yields accepted evidence.
/// # Errors
/// Rejects non-checkpoint/unsupported records before invoking the verifier,
/// propagates verification failure, and rejects substituted returned evidence.
pub fn verify_checkpoint_state(
    record: &TaskRecord,
    verifier: &dyn CheckpointStateVerifier,
) -> Result<CheckpointStateEvidence, CheckpointStateError> {
    checkpoint(record)?;
    if record.version != 1 {
        return Err(CheckpointStateError::BindingMismatch);
    }
    bind_checkpoint_state(record, verifier.verify(record)?)
}

/// Correlates independently verified state with an exact checkpoint event.
/// This validates identity, not cryptographic proof. Applications requiring
/// integrity must call their verifier before accepting its evidence here.
/// Prefer [`verify_checkpoint_state`] to perform both steps in one call.
/// # Errors
/// Rejects non-checkpoint records and foreign/stale checkpoint evidence.
pub fn bind_checkpoint_state(
    record: &TaskRecord,
    evidence: CheckpointStateEvidence,
) -> Result<CheckpointStateEvidence, CheckpointStateError> {
    let checkpoint = checkpoint(record)?;
    if record.version != 1
        || evidence.scope != record.scope
        || evidence.generation != record.generation
        || evidence.checkpoint != checkpoint
    {
        return Err(CheckpointStateError::BindingMismatch);
    }
    Ok(evidence)
}
