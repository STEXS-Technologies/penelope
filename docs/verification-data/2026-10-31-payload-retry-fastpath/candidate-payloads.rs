use crate::{
    SqliteProcessStore,
    process::{charge_process, db_error, port_error, scope_process_key},
    schema::execute_one,
    tasks::column_bytes,
};
use async_trait::async_trait;
use penelope_domain::{ContentDigest, ProcessScope};
use penelope_ports::{PortError, payloads::ProcessPayloadStore};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

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
                let key = scope.canonical_bytes();
                // An exact retry only reads committed evidence. Finish that read
                // before opening a write transaction, so unrelated writers do not
                // block retries. Never cache payload bytes or validation answers.
                if let Some(matches) =
                    stored_payload_matches(connection, &key, digest, &bytes, limits.max_blob_bytes)?
                {
                    return if matches {
                        Ok(digest)
                    } else {
                        Err(PortError::Invariant)
                    };
                }
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|error| db_error(&error))?;
                // Another writer may have inserted after our read. Recheck under
                // the writer lock before charging quota or inserting a new row.
                let stored = stored_payload_matches(
                    &transaction,
                    &key,
                    digest,
                    &bytes,
                    limits.max_blob_bytes,
                )?;
                if let Some(stored) = stored {
                    if !stored {
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
                    execute_one(
                        &transaction,
                        "INSERT INTO process_payloads VALUES (?1,?2,?3)",
                        params![key, digest.0.as_slice(), bytes],
                    )
                    .map_err(port_error)?;
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
                connection
                    .prepare_cached(
                        "SELECT bytes FROM process_payloads WHERE scope=?1 AND digest=?2",
                    )
                    .map_err(|error| db_error(&error))?
                    .query_row(
                        params![scope.canonical_bytes(), digest.0.as_slice()],
                        |row| {
                            let bytes = column_bytes(row, 0, limit)?;
                            // Invalid stored content has the same corruption
                            // classification as an invalid BLOB type or size.
                            if ContentDigest::sha256(bytes) != digest {
                                return Err(rusqlite::Error::InvalidQuery);
                            }
                            Ok(bytes.to_vec())
                        },
                    )
                    .optional()
                    .map_err(|error| db_error(&error))?
                    .ok_or(PortError::Invariant)
            })?
            .await
    }
}

/// Read current, bounded BLOB evidence; the statement is reset before returning.
fn stored_payload_matches(
    connection: &Connection,
    key: &[u8],
    digest: ContentDigest,
    bytes: &[u8],
    limit: u32,
) -> Result<Option<bool>, PortError> {
    connection
        .prepare_cached("SELECT bytes FROM process_payloads WHERE scope=?1 AND digest=?2")
        .map_err(|error| db_error(&error))?
        .query_row(params![key, digest.0.as_slice()], |row| {
            column_bytes(row, 0, limit).map(|stored| stored == bytes)
        })
        .optional()
        .map_err(|error| db_error(&error))
}

#[cfg(test)]
#[path = "payload_retry_tests.rs"]
mod tests;
