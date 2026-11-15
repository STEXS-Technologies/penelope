//! Same verifier as idempotent native commit retries, under a read transaction.
use super::*;
use penelope_ports::ProcessCommitVerifier;
#[async_trait]
impl ProcessCommitVerifier for SqliteProcessStore {
    async fn verify_commit(
        &self,
        commit: &AtomicProcessCommit,
    ) -> Result<AtomicProcessCommitReceipt, PortError> {
        commit.validate().map_err(|_commit| PortError::Invariant)?;
        let commit = commit.clone();
        let limits = self.limits;
        self.worker
            .run(move |connection| {
                let digest = batch_digest(&commit, limits)?;
                let key = commit
                    .outcomes
                    .first()
                    .ok_or(PortError::Invariant)?
                    .scope()
                    .canonical_bytes();
                let transaction = connection.transaction().map_err(|error| db_error(&error))?;
                committed_receipt(&transaction, &commit, &key, digest, limits)?
                    .ok_or(PortError::Conflict)
            })?
            .await
    }
}
