//! Read-only proof of a previously admitted native atomic process batch.
use crate::{AtomicProcessCommit, AtomicProcessCommitReceipt, PortError};
use async_trait::async_trait;
/// Optional stronger receipt lookup alongside `ProcessStore`. Verification must
/// authenticate the exact retained commit identity, frontier, outcomes, actions,
/// and input/source consumption when present. It must never submit, repair,
/// dispatch, acknowledge delivery or change native history. Missing/corrupt
/// constituents are errors, not permission to reconstruct a synthetic receipt.
#[async_trait]
pub trait ProcessCommitVerifier: Send + Sync {
    async fn verify_commit(
        &self,
        commit: &AtomicProcessCommit,
    ) -> Result<AtomicProcessCommitReceipt, PortError>;
}
