use crate::{
    SqliteProcessStore,
    process::{charge_process, db_error, scope_process_key},
    tasks::copy_column,
};
use async_trait::async_trait;
use penelope_domain::{ContentDigest, ProcessScope};
use penelope_ports::{PortError, payloads::ProcessPayloadStore};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

#[async_trait]
impl ProcessPayloadStore for SqliteProcessStore {
    async fn put_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
        bytes: &[u8],
    ) -> Result<ContentDigest, PortError> {
        if u32::try_from(bytes.len())
            .ok()
            .is_none_or(|length| length > self.limits.max_blob_bytes)
        {
            return Err(PortError::QuotaExceeded);
        }
        if ContentDigest::sha256(bytes) != digest {
            return Err(PortError::Invariant);
        }
        let bytes = bytes.to_vec();
        let scope = scope.clone();
        let limits = self.limits;
        self.worker
            .run(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|error| db_error(&error))?;
                let key = scope.canonical_bytes();
                let stored = transaction
                    .query_row(
                        "SELECT bytes FROM process_payloads WHERE scope=?1 AND digest=?2",
                        params![key, digest.0.as_slice()],
                        |row| copy_column(row, 0, limits.max_blob_bytes),
                    )
                    .optional()
                    .map_err(|error| db_error(&error))?;
                if let Some(stored) = stored {
                    if stored != bytes {
                        return Err(PortError::Invariant);
                    }
                } else {
                    let count: u32 = transaction
                        .query_row(
                            "SELECT count(*) FROM process_payloads WHERE scope=?1",
                            [&key],
                            |row| row.get(0),
                        )
                        .map_err(|error| db_error(&error))?;
                    if count >= limits.max_history_records {
                        return Err(PortError::QuotaExceeded);
                    }
                    let usage_key = scope_process_key(&scope)?;
                    let charge = u64::try_from(bytes.len())
                        .map_err(|_overflow| PortError::QuotaExceeded)?
                        .checked_add(
                            u64::try_from(key.len())
                                .map_err(|_overflow| PortError::QuotaExceeded)?,
                        )
                        .and_then(|size| size.checked_add(128))
                        .ok_or(PortError::QuotaExceeded)?;
                    charge_process(&transaction, &usage_key, charge, limits)?;
                    transaction
                        .execute(
                            "INSERT INTO process_payloads VALUES (?1,?2,?3)",
                            params![key, digest.0.as_slice(), bytes],
                        )
                        .map_err(|error| db_error(&error))?;
                }
                transaction.commit().map_err(|error| db_error(&error))?;
                Ok(digest)
            })?
            .await
    }

    async fn read_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
    ) -> Result<Vec<u8>, PortError> {
        let scope = scope.clone();
        let limit = self.limits.max_blob_bytes;
        self.worker
            .run(move |connection| {
                let bytes = connection
                    .query_row(
                        "SELECT bytes FROM process_payloads WHERE scope=?1 AND digest=?2",
                        params![scope.canonical_bytes(), digest.0.as_slice()],
                        |row| copy_column(row, 0, limit),
                    )
                    .optional()
                    .map_err(|error| db_error(&error))?
                    .ok_or(PortError::Invariant)?;
                if ContentDigest::sha256(&bytes) != digest {
                    return Err(PortError::Invariant);
                }
                Ok(bytes)
            })?
            .await
    }
}
