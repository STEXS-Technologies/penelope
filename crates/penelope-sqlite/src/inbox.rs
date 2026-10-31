use crate::{
    SqliteProcessStore,
    process::{db_error, decode, encode},
    tasks::column_bytes,
};
use async_trait::async_trait;
use penelope_domain::{ProcessId, ProcessInput, TenantId};
use penelope_ports::{
    PortError,
    inbox::{MAX_PENDING_INPUT_BATCH, PendingInbox},
};
use rusqlite::params;
use std::num::NonZeroU16;

#[async_trait]
impl PendingInbox for SqliteProcessStore {
    async fn pending_inputs(
        &self,
        tenant: &TenantId,
        process: &ProcessId,
        limit: NonZeroU16,
    ) -> Result<Vec<ProcessInput>, PortError> {
        if limit.get() > MAX_PENDING_INPUT_BATCH {
            return Err(PortError::QuotaExceeded);
        }
        let tenant = tenant.clone();
        let process = process.clone();
        let record_limit = self.limits.max_record_bytes;
        self.worker.run(move |connection| {
            let key = encode(&(&tenant, &process), 1024)?;
            let mut statement = connection.prepare_cached(
                "SELECT input_id,input,checksum FROM process_inbox WHERE process_key=?1 AND commit_digest IS NULL ORDER BY input_id LIMIT ?2"
            ).map_err(|error| db_error(&error))?;
            let mut rows = statement.query(params![key,limit.get()]).map_err(|error| db_error(&error))?;
            let mut inputs = Vec::new();
            while let Some(row) = rows.next().map_err(|error| db_error(&error))? {
                let identity = row.get_ref(0).map_err(|error| db_error(&error))?
                    .as_str().map_err(|_kind| PortError::Invariant)?;
                let bytes = column_bytes(row,1,record_limit).map_err(|error| db_error(&error))?;
                let checksum = column_bytes(row,2,32).map_err(|error| db_error(&error))?;
                let input: ProcessInput = decode(bytes,checksum)?;
                if input.tenant_id != tenant || input.process_id != process
                    || input.input_id.as_str() != identity || input.validate().is_err() {
                    return Err(PortError::Invariant);
                }
                if inputs.is_empty() { inputs.reserve_exact(usize::from(limit.get())); }
                inputs.push(input);
            }
            Ok(inputs)
        })?.await
    }
}
