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

/// Correlates independently verified state with an exact checkpoint event.
/// This validates identity, not cryptographic proof. Applications requiring
/// integrity must call their verifier before accepting its evidence here.
/// # Errors
/// Rejects non-checkpoint records and foreign/stale checkpoint evidence.
pub fn bind_checkpoint_state(
    record: &TaskRecord,
    evidence: CheckpointStateEvidence,
) -> Result<CheckpointStateEvidence, CheckpointStateError> {
    let checkpoint = match record.event {
        TaskExecutionEvent::CheckpointCommitted { checkpoint }
        | TaskExecutionEvent::CheckpointMigrated { checkpoint, .. } => checkpoint,
        TaskExecutionEvent::IntentCommitted { .. }
        | TaskExecutionEvent::Started
        | TaskExecutionEvent::FaultObserved { .. }
        | TaskExecutionEvent::ReconciliationRequired
        | TaskExecutionEvent::Reconciled { .. }
        | TaskExecutionEvent::RecoveryPlanned { .. }
        | TaskExecutionEvent::Restarted
        | TaskExecutionEvent::Completed { .. }
        | TaskExecutionEvent::RecoveryExhausted => return Err(CheckpointStateError::NotCheckpoint),
    };
    if record.version != 1
        || evidence.scope != record.scope
        || evidence.generation != record.generation
        || evidence.checkpoint != checkpoint
    {
        return Err(CheckpointStateError::BindingMismatch);
    }
    Ok(evidence)
}
