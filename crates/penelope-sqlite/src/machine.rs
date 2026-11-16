//! Scoped local journal over the existing bounded SQLite worker.
use crate::{
    SqliteLimits,
    schema::{execute_one, open, sql_error},
    tasks::column_bytes,
    worker::DatabaseWorker,
};
use async_trait::async_trait;
use penelope_core::{
    machine::*,
    task::{Digest, runtime::TaskPortError},
};
use penelope_ports::{PortError, machine::*};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{path::Path, sync::Arc};
mod delivery;
mod read;
pub use delivery::SqliteMachineOutbox;
use read::*;

/// Machine state journal. Construction/migration is synchronous; all port calls
/// use the shared bounded database worker. Policy is injected, with no allow-all
/// default. This adapter does not execute, lease or acknowledge external effects.
pub struct SqliteMachineStore<A: MachineAuthorizer, const INTENTS: usize> {
    worker: DatabaseWorker,
    authorizer: A,
    limits: SqliteLimits,
    payloads: MachinePayloadLimits,
}
const fn task_error(error: TaskPortError) -> MachineStoreError {
    match error {
        TaskPortError::Corrupt => MachineStoreError::Corrupt,
        TaskPortError::Conflict => MachineStoreError::Conflict,
        TaskPortError::Exhausted => MachineStoreError::Capacity,
        TaskPortError::Unavailable | TaskPortError::Runtime => MachineStoreError::Unavailable,
    }
}
const fn db_error(error: &rusqlite::Error) -> MachineStoreError {
    task_error(sql_error(error))
}
const fn worker_error(error: PortError) -> MachineStoreError {
    match error {
        PortError::QuotaExceeded => MachineStoreError::Capacity,
        PortError::Unavailable
        | PortError::Conflict
        | PortError::Unauthorized
        | PortError::TimedOut
        | PortError::Cancelled
        | PortError::Ambiguous
        | PortError::Invariant => MachineStoreError::Unavailable,
    }
}
fn write(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<(), MachineStoreError> {
    execute_one(connection, sql, params).map_err(task_error)
}
const fn key(scope: MachineScope) -> [u8; 64] {
    let mut bytes = [0; 64];
    let (tenant, instance) = bytes.split_at_mut(32);
    tenant.copy_from_slice(&scope.tenant.bytes());
    instance.copy_from_slice(&scope.instance.bytes());
    bytes
}
fn scope_bytes(scope: MachineScope) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(104);
    bytes.extend_from_slice(&key(scope));
    bytes.extend_from_slice(&scope.definition.fingerprint.bytes());
    bytes.extend_from_slice(&scope.definition.state_schema.to_le_bytes());
    bytes.extend_from_slice(&scope.definition.event_schema.to_le_bytes());
    bytes
}
fn integer(value: usize) -> Result<i64, MachineStoreError> {
    i64::try_from(value).map_err(|_invalid| MachineStoreError::Capacity)
}
fn payload_limit(limit: usize) -> Result<u32, MachineStoreError> {
    u32::try_from(limit).map_err(|_invalid| MachineStoreError::Capacity)
}
impl<A: MachineAuthorizer, const INTENTS: usize> SqliteMachineStore<A, INTENTS> {
    /// Open/migrate outside a latency-sensitive reactor. Disk/page and worker
    /// limits use the same connection setup as existing task/process adapters.
    /// # Errors
    /// Refuses unsupported storage, inconsistent limits or worker creation errors.
    pub fn open(
        path: &Path,
        limits: SqliteLimits,
        payloads: MachinePayloadLimits,
        authorizer: A,
        queue_capacity: u16,
    ) -> Result<Self, MachineStoreError> {
        if payloads.max_record_bytes < 329
            || payloads.max_record_bytes > limits.max_record_bytes as usize
            || payloads.max_state_bytes > limits.max_blob_bytes as usize
            || payloads.max_event_bytes > limits.max_blob_bytes as usize
            || payloads.max_intent_bytes > limits.max_blob_bytes as usize
            || payloads.max_batch_bytes as u128 > u128::from(limits.max_scope_bytes)
        {
            return Err(MachineStoreError::Capacity);
        }
        let connection = open(path, limits).map_err(task_error)?;
        Ok(Self {
            worker: DatabaseWorker::new(connection, queue_capacity).map_err(worker_error)?,
            limits,
            payloads,
            authorizer,
        })
    }
    pub const fn queue_capacity(&self) -> u16 {
        self.worker.capacity()
    }
}

#[async_trait]
impl<A: MachineAuthorizer, const INTENTS: usize> MachineStore<INTENTS>
    for SqliteMachineStore<A, INTENTS>
{
    type Context = A::Context;
    async fn bootstrap(
        &self,
        context: &Self::Context,
        request: Arc<PreparedMachineBootstrap>,
    ) -> Result<(), MachineStoreError> {
        self.authorizer
            .authorize(
                context,
                request.scope(),
                MachineOperation::Bootstrap {
                    state_digest: request.digest(),
                },
            )
            .await?;
        let limits = self.limits;
        let payloads = self.payloads;
        self.worker
            .run(move |connection| Ok(bootstrap(connection, &request, limits, payloads)))
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn commit(
        &self,
        context: &Self::Context,
        batch: Arc<MachineCommitBatch<INTENTS>>,
    ) -> Result<MachineCommitReceipt, MachineStoreError> {
        let header = batch.record().header();
        let expected = header.transition.context.expected;
        self.authorizer
            .authorize(
                context,
                expected.scope,
                MachineOperation::Commit {
                    expected: expected.revision,
                    event: header.transition.context.event,
                    identity: batch.record().identity(),
                },
            )
            .await?;
        let limits = self.limits;
        let payloads = self.payloads;
        self.worker
            .run(move |connection| Ok(commit(connection, &batch, limits, payloads)))
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn load_head(
        &self,
        context: &Self::Context,
        scope: MachineScope,
    ) -> Result<Option<MachineSnapshot>, MachineStoreError> {
        self.authorizer
            .authorize(context, scope, MachineOperation::ReadHead)
            .await?;
        let payloads = self.payloads;
        self.worker
            .run(move |connection| Ok(read_head::<INTENTS>(connection, scope, payloads)))
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn lookup(
        &self,
        context: &Self::Context,
        scope: MachineScope,
        event: MachineEventId,
    ) -> Result<Option<MachineStoredCommit<INTENTS>>, MachineStoreError> {
        self.authorizer
            .authorize(context, scope, MachineOperation::Lookup { event })
            .await?;
        let payloads = self.payloads;
        self.worker
            .run(move |connection| Ok(lookup(connection, scope, event, payloads)))
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
    async fn history(
        &self,
        context: &Self::Context,
        request: MachineHistoryRequest,
    ) -> Result<MachineHistoryPage<INTENTS>, MachineStoreError> {
        self.authorizer
            .authorize(
                context,
                request.scope,
                MachineOperation::ReadHistory {
                    after: request.after,
                    through: request.through,
                },
            )
            .await?;
        let payloads = self.payloads;
        let limits = self.limits;
        self.worker
            .run(move |connection| Ok(history(connection, request, limits, payloads)))
            .map_err(worker_error)?
            .await
            .map_err(worker_error)?
    }
}

fn bootstrap(
    connection: &mut Connection,
    request: &PreparedMachineBootstrap,
    limits: SqliteLimits,
    payloads: MachinePayloadLimits,
) -> Result<(), MachineStoreError> {
    if request.state().len() > payloads.max_state_bytes
        || request.state().len() > payloads.max_batch_bytes
    {
        return Err(MachineStoreError::Capacity);
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| db_error(&e))?;
    let existing = transaction
        .query_row(
            "SELECT scope,genesis,genesis_digest FROM machine_heads WHERE machine_key=?1",
            [key(request.scope()).as_slice()],
            |row| {
                Ok((
                    column_bytes(row, 0, 104)?.to_vec(),
                    column_bytes(
                        row,
                        1,
                        payload_limit(payloads.max_state_bytes)
                            .map_err(|_invalid| rusqlite::Error::InvalidQuery)?,
                    )?
                    .to_vec(),
                    column_bytes(row, 2, 32)?.to_vec(),
                ))
            },
        )
        .optional()
        .map_err(|e| db_error(&e))?;
    if let Some((scope, genesis, digest)) = existing {
        if scope != scope_bytes(request.scope()) {
            return Err(MachineStoreError::ScopeMismatch);
        }
        if Digest::of(&genesis).0.as_slice() != digest {
            return Err(MachineStoreError::Corrupt);
        }
        if genesis != request.state() || digest != request.digest() {
            return Err(MachineStoreError::Conflict);
        }
        return Ok(());
    }
    let count: u32 = transaction
        .query_row("SELECT count(*) FROM machine_heads", [], |row| row.get(0))
        .map_err(|e| db_error(&e))?;
    let used = request
        .state()
        .len()
        .checked_mul(2)
        .ok_or(MachineStoreError::Capacity)?;
    if count >= limits.max_scopes || used as u128 > u128::from(limits.max_scope_bytes) {
        return Err(MachineStoreError::Capacity);
    }
    write(
        &transaction,
        "INSERT INTO machine_heads(machine_key,scope,revision,genesis,genesis_digest,state,state_digest,last_event,bytes_used,event_count) VALUES(?1,?2,?3,?4,?5,?4,?5,NULL,?6,0)",
        params![
            key(request.scope()).as_slice(),
            scope_bytes(request.scope()),
            0_u64.to_be_bytes().as_slice(),
            request.state(),
            request.digest().as_slice(),
            integer(used)?
        ],
    )?;
    let stored = read_head_in::<0>(&transaction, request.scope(), payloads)?
        .ok_or(MachineStoreError::Corrupt)?;
    if stored.state() != request.state() || stored.digest() != request.digest() {
        return Err(MachineStoreError::Corrupt);
    }
    transaction.commit().map_err(|e| db_error(&e))
}

enum CommitAdmission {
    Existing(MachineCommitReceipt),
    New(MachineSnapshot),
}

fn admission<const N: usize>(
    connection: &Connection,
    batch: &MachineCommitBatch<N>,
    payloads: MachinePayloadLimits,
) -> Result<CommitAdmission, MachineStoreError> {
    let header = batch.record().header();
    let expected = header.transition.context.expected;
    let head = read_head_in::<N>(connection, expected.scope, payloads)?
        .ok_or(MachineStoreError::Conflict)?;
    if let Some(stored) = read_commit::<N>(
        connection,
        expected.scope,
        header.transition.context.event,
        payloads,
    )? {
        let previous = stored.batch();
        if previous.record().identity() != batch.record().identity()
            || previous.record().header() != batch.record().header()
            || previous.event() != batch.event()
            || previous.state() != batch.state()
            || !previous.intents().eq(batch.intents())
        {
            return Err(MachineStoreError::Conflict);
        }
        return Ok(CommitAdmission::Existing(stored.receipt()));
    }
    if head.head() != expected {
        return Err(MachineStoreError::StaleHead);
    }
    if head.digest() != header.previous_state_digest {
        return Err(MachineStoreError::Conflict);
    }
    Ok(CommitAdmission::New(head))
}

fn commit<const N: usize>(
    connection: &mut Connection,
    batch: &MachineCommitBatch<N>,
    limits: SqliteLimits,
    payloads: MachinePayloadLimits,
) -> Result<MachineCommitReceipt, MachineStoreError> {
    let header = batch.record().header();
    let expected = header.transition.context.expected;
    if batch.byte_len() > payloads.max_batch_bytes
        || batch.record().encoded_len() > payloads.max_record_bytes
        || batch.event().len() > payloads.max_event_bytes
        || batch.state().len() > payloads.max_state_bytes
        || batch
            .intents()
            .any(|(_, bytes)| bytes.len() > payloads.max_intent_bytes)
    {
        return Err(MachineStoreError::Capacity);
    }
    #[cfg(all(test, unix))]
    tests::cut("before-transaction");
    // A read snapshot may resolve an immutable duplicate or an already stale
    // head without joining the writer queue. The query is only a hint: the same
    // complete validation runs here and again under the write lock for new work.
    {
        let snapshot = connection.transaction().map_err(|e| db_error(&e))?;
        let resolved: bool = snapshot
            .prepare_cached(
                "SELECT EXISTS(SELECT 1 FROM machine_heads WHERE machine_key=?1 AND revision<>?2)",
            )
            .map_err(|e| db_error(&e))?
            .query_row(
                params![
                    key(expected.scope).as_slice(),
                    expected.revision.get().to_be_bytes().as_slice()
                ],
                |row| row.get(0),
            )
            .map_err(|e| db_error(&e))?;
        if resolved {
            match admission(&snapshot, batch, payloads)? {
                CommitAdmission::Existing(receipt) => return Ok(receipt),
                CommitAdmission::New(_) => {}
            }
        }
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| db_error(&e))?;
    #[cfg(all(test, unix))]
    tests::cut("after-begin");
    let head = match admission(&transaction, batch, payloads)? {
        CommitAdmission::Existing(receipt) => return Ok(receipt),
        CommitAdmission::New(head) => head,
    };
    let (used, count): (i64, u32) = transaction
        .query_row(
            "SELECT bytes_used,event_count FROM machine_heads WHERE machine_key=?1",
            [key(expected.scope).as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| db_error(&e))?;
    // Retain exact event snapshots plus the current head; charge the replaced
    // head state only once. No automatic history eviction can erase dedupe IDs.
    if u64::from(count) != expected.revision.get() {
        return Err(MachineStoreError::Corrupt);
    }
    let retained = u64::try_from(used)
        .ok()
        .and_then(|bytes| bytes.checked_sub(u64::try_from(head.state().len()).ok()?))
        .ok_or(MachineStoreError::Corrupt)?;
    let next_used = retained
        .checked_add(
            u64::try_from(batch.byte_len()).map_err(|_invalid| MachineStoreError::Capacity)?,
        )
        .and_then(|bytes| bytes.checked_add(u64::try_from(batch.state().len()).ok()?))
        .filter(|bytes| *bytes <= limits.max_scope_bytes)
        .ok_or(MachineStoreError::Capacity)?;
    if next_used
        .checked_add(delivery::usage(&transaction, expected.scope)?)
        .is_none_or(|bytes| bytes > limits.max_scope_bytes)
    {
        return Err(MachineStoreError::Capacity);
    }
    if count >= limits.max_history_records {
        return Err(MachineStoreError::Capacity);
    }
    let mut record = vec![0; batch.record().encoded_len()];
    batch
        .record()
        .encode(&mut record)
        .map_err(|_invalid| MachineStoreError::Corrupt)?;
    let machine_key = key(expected.scope);
    let event = header.transition.context.event.bytes();
    write(
        &transaction,
        "INSERT INTO machine_events(machine_key,event,revision,record,event_payload,state) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            machine_key.as_slice(),
            event.as_slice(),
            header.transition.revision.get().to_be_bytes().as_slice(),
            record,
            batch.event(),
            batch.state()
        ],
    )?;
    #[cfg(all(test, unix))]
    tests::cut("after-event");
    for (ordinal, (metadata, bytes)) in batch.intents().enumerate() {
        write(
            &transaction,
            "INSERT INTO machine_intents(machine_key,intent,event,ordinal,payload) VALUES(?1,?2,?3,?4,?5)",
            params![
                machine_key.as_slice(),
                metadata.identity.bytes().as_slice(),
                event.as_slice(),
                integer(ordinal)?,
                bytes
            ],
        )?;
    }
    #[cfg(all(test, unix))]
    tests::cut("after-intents");
    write(
        &transaction,
        "UPDATE machine_heads SET revision=?2,state=?3,state_digest=?4,last_event=?5,bytes_used=?6,event_count=event_count+1 WHERE machine_key=?1 AND revision=?7",
        params![
            machine_key.as_slice(),
            header.transition.revision.get().to_be_bytes().as_slice(),
            batch.state(),
            header.next_state_digest.as_slice(),
            event.as_slice(),
            i64::try_from(next_used).map_err(|_invalid| MachineStoreError::Capacity)?,
            expected.revision.get().to_be_bytes().as_slice()
        ],
    )?;
    #[cfg(all(test, unix))]
    tests::cut("after-head");
    let verified = read_head_in::<N>(&transaction, expected.scope, payloads)?
        .ok_or(MachineStoreError::Corrupt)?;
    let receipt = MachineCommitReceipt {
        head: MachineHead {
            scope: expected.scope,
            revision: header.transition.revision,
        },
        event: header.transition.context.event,
        identity: batch.record().identity(),
        disposition: MachineCommitDisposition::AlreadyCommitted,
    };
    let stored_usage: i64 = transaction
        .query_row(
            "SELECT bytes_used FROM machine_heads WHERE machine_key=?1",
            [machine_key.as_slice()],
            |row| row.get(0),
        )
        .map_err(|e| db_error(&e))?;
    if verified.last() != Some(receipt)
        || verified.state() != batch.state()
        || u64::try_from(stored_usage).ok() != Some(next_used)
    {
        return Err(MachineStoreError::Corrupt);
    }
    #[cfg(all(test, unix))]
    tests::cut("after-validation");
    transaction.commit().map_err(|e| db_error(&e))?;
    #[cfg(all(test, unix))]
    tests::cut("after-commit");
    Ok(MachineCommitReceipt {
        disposition: MachineCommitDisposition::Committed,
        ..receipt
    })
}

#[cfg(all(test, unix))]
mod tests;
