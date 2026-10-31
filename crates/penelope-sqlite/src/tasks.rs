use crate::schema::{SqliteLimits, execute_one, open, sql_error};
use penelope_core::task::{runtime::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::Path;

/// Synchronous WAL/FULL journal pinned to one task definition. A connection
/// caches one validated scope; switching scopes or observing another writer's
/// commit triggers bounded replay, even if the head is unchanged. No history
/// scan occurs on uncontended append.
pub struct SqliteTaskJournal {
    pub(crate) connection: Connection,
    pub(crate) definition: TaskDefinition,
    pub(crate) limits: SqliteLimits,
    pub(crate) cache: Option<(TaskProjection, Digest, u64, i64)>,
}

// Sample inside BEGIN IMMEDIATE, while no other writer can commit. Values
// belong to this connection; its own commits do not advance this counter.
fn data_version(connection: &Connection) -> Result<i64, TaskPortError> {
    connection
        .prepare_cached("PRAGMA main.data_version")
        .map_err(|error| sql_error(&error))?
        .query_row([], |row| row.get(0))
        .map_err(|error| sql_error(&error))
}

pub(crate) fn key(scope: TaskScope) -> [u8; 128] {
    let mut key = [0; 128];
    for (destination, digest) in key.chunks_exact_mut(32).zip([
        scope.tenant.digest(),
        scope.task.digest(),
        scope.definition.digest(),
        scope.input.digest(),
    ]) {
        destination.copy_from_slice(&digest.0);
    }
    key
}

pub(crate) fn column_bytes<'row>(
    row: &'row rusqlite::Row<'_>,
    index: usize,
    limit: u32,
) -> rusqlite::Result<&'row [u8]> {
    let bytes = row.get_ref(index)?.as_blob()?;
    if u32::try_from(bytes.len())
        .ok()
        .is_none_or(|len| len > limit)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(bytes)
}

pub(crate) fn copy_column(
    row: &rusqlite::Row<'_>,
    index: usize,
    limit: u32,
) -> rusqlite::Result<Vec<u8>> {
    column_bytes(row, index, limit).map(<[u8]>::to_vec)
}

fn decode(
    bytes: &[u8],
    checksum: &[u8],
    scope: TaskScope,
    sequence: TaskSequence,
) -> Result<TaskRecord, TaskPortError> {
    if Digest::of(bytes).0.as_slice() != checksum {
        return Err(TaskPortError::Corrupt);
    }
    let record: TaskRecord =
        serde_json::from_slice(bytes).map_err(|_invalid| TaskPortError::Corrupt)?;
    if record.scope != scope || record.sequence != sequence || record.version != 1 {
        return Err(TaskPortError::Corrupt);
    }
    Ok(record)
}

const fn reference(record: &TaskRecord) -> Option<BlobRef> {
    match record.event {
        TaskExecutionEvent::IntentCommitted { input, .. } => Some(input),
        TaskExecutionEvent::CheckpointCommitted { checkpoint }
        | TaskExecutionEvent::CheckpointMigrated { checkpoint, .. } => Some(checkpoint),
        TaskExecutionEvent::Completed { output }
        | TaskExecutionEvent::Reconciled {
            result: Reconciliation::Completed { output, .. },
        } => Some(output),
        TaskExecutionEvent::Started
        | TaskExecutionEvent::FaultObserved { .. }
        | TaskExecutionEvent::ReconciliationRequired
        | TaskExecutionEvent::Reconciled { .. }
        | TaskExecutionEvent::RecoveryPlanned { .. }
        | TaskExecutionEvent::Restarted
        | TaskExecutionEvent::RecoveryExhausted => None,
    }
}

fn with_verified_blob<T>(
    connection: &Connection,
    scope: TaskScope,
    blob: BlobRef,
    limit: u32,
    consume: impl FnOnce(&[u8]) -> Result<T, TaskPortError>,
) -> Result<T, TaskPortError> {
    if blob.bytes > limit {
        return Err(TaskPortError::Exhausted);
    }
    connection
        .query_row(
            "SELECT bytes FROM task_blobs WHERE scope=?1 AND digest=?2",
            params![key(scope), blob.digest.0.as_slice()],
            |row| {
                let bytes = column_bytes(row, 0, limit)?;
                Ok(blob
                    .verify(bytes, blob.schema)
                    .map_err(|_invalid| TaskPortError::Corrupt)
                    .and_then(|()| consume(bytes)))
            },
        )
        .optional()
        .map_err(|error| sql_error(&error))?
        .ok_or(TaskPortError::Corrupt)?
}

pub(crate) fn reconstruct(
    connection: &Connection,
    scope: TaskScope,
    definition: TaskDefinition,
    limits: SqliteLimits,
) -> Result<(TaskProjection, Digest, u64), TaskPortError> {
    let mut projection =
        TaskProjection::new(scope, definition).map_err(|_invalid| TaskPortError::Corrupt)?;
    let mut statement = connection.prepare("SELECT sequence,record,checksum FROM task_records WHERE scope=?1 ORDER BY sequence LIMIT ?2").map_err(|error| sql_error(&error))?;
    let mut rows = statement
        .query(params![
            key(scope),
            i64::from(limits.max_history_records).saturating_add(1)
        ])
        .map_err(|error| sql_error(&error))?;
    let mut last = Digest([0; 32]);
    let mut used = 0_u64;
    while let Some(row) = rows.next().map_err(|error| sql_error(&error))? {
        if projection.next_sequence().get() >= limits.max_history_records {
            return Err(TaskPortError::Exhausted);
        }
        let sequence: u32 = row.get(0).map_err(|_invalid| TaskPortError::Corrupt)?;
        let bytes = column_bytes(row, 1, limits.max_record_bytes)
            .map_err(|_invalid| TaskPortError::Corrupt)?;
        let checksum = column_bytes(row, 2, 32).map_err(|_invalid| TaskPortError::Corrupt)?;
        let record = decode(bytes, checksum, scope, TaskSequence::new(sequence))?;
        projection
            .apply(&record)
            .map_err(|_invalid| TaskPortError::Corrupt)?;
        if let Some(blob) = reference(&record) {
            with_verified_blob(connection, scope, blob, limits.max_blob_bytes, |_bytes| {
                Ok(())
            })?;
        }
        // decode verified this checksum against the exact encoded bytes.
        last = Digest(
            checksum
                .try_into()
                .map_err(|_invalid| TaskPortError::Corrupt)?,
        );
        used = used
            .checked_add(u64::try_from(bytes.len()).map_err(|_invalid| TaskPortError::Exhausted)?)
            .ok_or(TaskPortError::Exhausted)?;
    }
    let blob_bytes: u64 = connection
        .query_row(
            "SELECT COALESCE(SUM(length(bytes)),0) FROM task_blobs WHERE scope=?1",
            [key(scope)],
            |row| nonnegative(row, 0),
        )
        .map_err(|error| sql_error(&error))?;
    used = used
        .checked_add(blob_bytes)
        .ok_or(TaskPortError::Exhausted)?;
    if used > limits.max_scope_bytes {
        return Err(TaskPortError::Exhausted);
    }
    Ok((projection, last, used))
}

fn head(
    connection: &Connection,
    scope: TaskScope,
) -> Result<Option<(u32, Vec<u8>, u64)>, TaskPortError> {
    connection
        .query_row(
            "SELECT next_sequence,last_digest,bytes_used FROM task_heads WHERE scope=?1",
            [key(scope)],
            |row| Ok((row.get(0)?, copy_column(row, 1, 32)?, nonnegative(row, 2)?)),
        )
        .optional()
        .map_err(|error| sql_error(&error))
}

// Check the retained tail in the caller's transaction snapshot, including
// before trusting a cached projection for append. MAX uses the scoped primary
// key; this does not replay the history or cache query results.
fn checked_head(
    connection: &Connection,
    scope: TaskScope,
) -> Result<Option<(u32, Digest, u64)>, TaskPortError> {
    let (next, checksum, used, last): (Option<u32>, Option<Digest>, Option<i64>, Option<u32>) = connection
        .prepare_cached("SELECT head.next_sequence,head.last_digest,head.bytes_used,(SELECT max(sequence) FROM task_records WHERE scope=?1) FROM (SELECT 1) LEFT JOIN task_heads AS head ON head.scope=?1")
        .map_err(|error| sql_error(&error))?
        .query_row([key(scope)], |row| {
            let checksum = if row.get_ref(1)? == rusqlite::types::ValueRef::Null {
                None
            } else {
                Some(Digest(column_bytes(row, 1, 32)?.try_into().map_err(|_invalid| rusqlite::Error::InvalidQuery)?))
            };
            Ok((row.get(0)?, checksum, row.get(2)?, row.get(3)?))
        })
        .map_err(|error| sql_error(&error))?;
    match (next, checksum, used, last) {
        (None, None, None, None) => Ok(None),
        (Some(next), Some(checksum), Some(used), Some(last))
            if next > 0 && last.checked_add(1) == Some(next) =>
        {
            Ok(Some((
                next,
                checksum,
                u64::try_from(used).map_err(|_invalid| TaskPortError::Corrupt)?,
            )))
        }
        _ => Err(TaskPortError::Corrupt),
    }
}

impl SqliteTaskJournal {
    /// Opens/migrates a local database, enforcing WAL/FULL and bounded admission.
    /// # Errors
    /// Returns invalid limits/definition, unsupported schema or SQLite failures.
    pub fn open(
        path: &Path,
        definition: TaskDefinition,
        limits: SqliteLimits,
    ) -> Result<Self, TaskPortError> {
        definition
            .validate()
            .map_err(|_invalid| TaskPortError::Corrupt)?;
        if definition.budget.max_input_bytes > limits.max_blob_bytes
            || definition.budget.max_output_bytes > limits.max_blob_bytes
            || definition.recovery.max_checkpoint_bytes > limits.max_blob_bytes
            || definition.budget.max_history_records > limits.max_history_records
        {
            return Err(TaskPortError::Exhausted);
        }
        Ok(Self {
            connection: open(path, limits)?,
            definition,
            limits,
            cache: None,
        })
    }

    /// Revalidates the immutable log and its blobs, rebuilding only derived head
    /// metadata. Corrupt/gapped histories are retained and rejected. Requires
    /// exclusive application maintenance ownership, never use to hide data loss.
    /// # Errors
    /// Returns corrupt records/blobs, exhausted bounds or database failures.
    pub fn rebuild_head(&mut self, scope: TaskScope) -> Result<(), TaskPortError> {
        self.cache = None;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error(&error))?;
        reject_retired(&transaction, scope)?;
        let (projection, last, used) =
            reconstruct(&transaction, scope, self.definition, self.limits)?;
        if projection.next_sequence() == TaskSequence::ZERO {
            return Err(TaskPortError::Corrupt);
        }
        // A known durable frontier must never move backwards (or silently
        // adopt an unexplained tail). Missing records require operator repair
        // from authoritative evidence, not a derived-cache rebuild.
        if head(&transaction, scope)?
            .is_some_and(|(next, _, _)| next != projection.next_sequence().get())
        {
            return Err(TaskPortError::Corrupt);
        }
        execute_one(
            &transaction,
            "INSERT INTO task_heads VALUES (?1,?2,?3,?4) ON CONFLICT(scope) DO UPDATE SET next_sequence=excluded.next_sequence,last_digest=excluded.last_digest,bytes_used=excluded.bytes_used",
            params![
                key(scope),
                projection.next_sequence().get(),
                last.0.as_slice(),
                sql_integer(used)?
            ],
        )?;
        let version = data_version(&transaction)?;
        transaction.commit().map_err(|error| sql_error(&error))?;
        self.cache = Some((projection, last, used, version));
        Ok(())
    }
}

impl TaskJournal for SqliteTaskJournal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        if scope.definition != self.definition.fingerprint() {
            return Err(TaskPortError::Conflict);
        }
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| sql_error(&error))?;
        reject_retired(&transaction, scope)?;
        let head = checked_head(&transaction, scope)?;
        let stored = transaction
            .query_row(
                "SELECT record,checksum FROM task_records WHERE scope=?1 AND sequence=?2",
                params![key(scope), sequence.get()],
                |row| {
                    Ok(decode(
                        column_bytes(row, 0, self.limits.max_record_bytes)?,
                        column_bytes(row, 1, 32)?,
                        scope,
                        sequence,
                    ))
                },
            )
            .optional()
            .map_err(|error| sql_error(&error))?;
        match (head, stored) {
            (Some((next, _, _)), Some(record)) if sequence.get() < next => record.map(Some),
            (Some((next, _, _)), None) if sequence.get() >= next => Ok(None),
            (None, None) => Ok(None),
            _ => Err(TaskPortError::Corrupt),
        }
    }

    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        if record.scope.definition != self.definition.fingerprint() {
            return Err(TaskPortError::Conflict);
        }
        if record.sequence.get() >= self.limits.max_history_records {
            return Err(TaskPortError::Exhausted);
        }
        if blob.is_some_and(|bytes| {
            u32::try_from(bytes.len())
                .ok()
                .is_none_or(|len| len > self.limits.max_blob_bytes)
        }) {
            return Err(TaskPortError::Exhausted);
        }
        let encoded = serde_json::to_vec(record).map_err(|_invalid| TaskPortError::Corrupt)?;
        if u32::try_from(encoded.len())
            .ok()
            .is_none_or(|len| len > self.limits.max_record_bytes)
        {
            return Err(TaskPortError::Exhausted);
        }
        let digest = Digest::of(&encoded);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error(&error))?;
        reject_retired(&transaction, record.scope)?;
        let stored_head = checked_head(&transaction, record.scope)?;
        let version = data_version(&transaction)?;
        let mut projection = match (&self.cache, &stored_head) {
            (
                Some((cached, last, cached_used, cached_version)),
                Some((next, checksum, stored_used)),
            ) if cached.scope() == record.scope
                && cached.next_sequence().get() == *next
                && last == checksum
                && cached_used == stored_used
                && *cached_version == version =>
            {
                *cached
            }
            _ => {
                let (projection, last, used) =
                    reconstruct(&transaction, record.scope, self.definition, self.limits)?;
                match &stored_head {
                    Some((next, checksum, stored_used))
                        if *next == projection.next_sequence().get()
                            && checksum == &last
                            && *stored_used == used => {}
                    None if projection.next_sequence() == TaskSequence::ZERO => {}
                    _ => return Err(TaskPortError::Corrupt),
                }
                projection
            }
        };
        projection
            .apply(record)
            .map_err(|_invalid| TaskPortError::Conflict)?;
        let mut added =
            u64::try_from(encoded.len()).map_err(|_invalid| TaskPortError::Exhausted)?;
        match (reference(record), blob) {
            (Some(reference), Some(bytes)) => {
                reference
                    .verify(bytes, reference.schema)
                    .map_err(|_invalid| TaskPortError::Corrupt)?;
                let inserted = transaction.execute("INSERT INTO task_blobs VALUES (?1,?2,?3) ON CONFLICT(scope,digest) DO NOTHING", params![key(record.scope), reference.digest.0.as_slice(), bytes]).map_err(|error| sql_error(&error))?;
                if inserted == 1 {
                    added = added
                        .checked_add(u64::from(reference.bytes))
                        .ok_or(TaskPortError::Exhausted)?;
                } else {
                    // The caller bytes were verified above. Exact equality with
                    // that buffer also verifies the stored length and digest;
                    // hashing the stored bytes again would duplicate that work.
                    let matches = transaction
                        .query_row(
                            "SELECT bytes FROM task_blobs WHERE scope=?1 AND digest=?2",
                            params![key(record.scope), reference.digest.0.as_slice()],
                            |row| Ok(column_bytes(row, 0, self.limits.max_blob_bytes)? == bytes),
                        )
                        .optional()
                        .map_err(|error| sql_error(&error))?;
                    if matches != Some(true) {
                        return Err(TaskPortError::Corrupt);
                    }
                }
            }
            (None, None) => {}
            _ => return Err(TaskPortError::Corrupt),
        }
        let used = stored_head
            .as_ref()
            .map_or(0, |(_, _, used)| *used)
            .checked_add(added)
            .ok_or(TaskPortError::Exhausted)?;
        if used > self.limits.max_scope_bytes {
            return Err(TaskPortError::Exhausted);
        }
        match stored_head {
            Some((next, checksum, _)) => {
                let changed = transaction.execute("UPDATE task_heads SET next_sequence=?1,last_digest=?2,bytes_used=?3 WHERE scope=?4 AND next_sequence=?5 AND last_digest=?6", params![projection.next_sequence().get(), digest.0.as_slice(), sql_integer(used)?, key(record.scope), next, checksum.0.as_slice()]).map_err(|error| sql_error(&error))?;
                if changed != 1 {
                    return Err(TaskPortError::Conflict);
                }
            }
            None => {
                let count: u32 = transaction
                    .query_row("SELECT (SELECT count(*) FROM task_heads)+(SELECT count(*) FROM task_retirements)", [], |row| row.get(0))
                    .map_err(|error| sql_error(&error))?;
                if count >= self.limits.max_scopes {
                    return Err(TaskPortError::Exhausted);
                }
                execute_one(
                    &transaction,
                    "INSERT INTO task_heads VALUES (?1,?2,?3,?4)",
                    params![
                        key(record.scope),
                        projection.next_sequence().get(),
                        digest.0.as_slice(),
                        sql_integer(used)?
                    ],
                )?;
            }
        }
        execute_one(
            &transaction,
            "INSERT INTO task_records VALUES (?1,?2,?3,?4)",
            params![
                key(record.scope),
                record.sequence.get(),
                encoded,
                digest.0.as_slice()
            ],
        )?;
        transaction.commit().map_err(|error| sql_error(&error))?;
        self.cache = Some((projection, digest, used, version));
        Ok(*record)
    }

    fn read_blob(
        &mut self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        if scope.definition != self.definition.fingerprint() {
            return Err(TaskPortError::Conflict);
        }
        if usize::try_from(blob.bytes)
            .ok()
            .is_none_or(|len| len > buffer.len())
        {
            return Err(TaskPortError::Exhausted);
        }
        reject_retired(&self.connection, scope)?;
        with_verified_blob(
            &self.connection,
            scope,
            blob,
            self.limits.max_blob_bytes,
            |bytes| {
                buffer
                    .get_mut(..bytes.len())
                    .ok_or(TaskPortError::Exhausted)?
                    .copy_from_slice(bytes);
                Ok(bytes.len())
            },
        )
    }
}

pub(crate) fn optional_digest(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<Digest>> {
    if matches!(row.get_ref(index)?, rusqlite::types::ValueRef::Null) {
        return Ok(None);
    }
    let digest = column_bytes(row, index, 32)?
        .try_into()
        .map_err(|_invalid| rusqlite::Error::InvalidQuery)?;
    Ok(Some(Digest(digest)))
}

fn nonnegative(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_invalid| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
fn sql_integer(value: u64) -> Result<i64, TaskPortError> {
    i64::try_from(value).map_err(|_invalid| TaskPortError::Exhausted)
}

pub(crate) fn authorized_projection(
    connection: &Connection,
    scope: TaskScope,
    definition: TaskDefinition,
    limits: SqliteLimits,
) -> Result<TaskProjection, TaskPortError> {
    let (projection, digest, used) = reconstruct(connection, scope, definition, limits)?;
    // Compare the digest while borrowed. Maintenance keeps its separate head
    // decoder so an otherwise valid frontier can still have its digest repaired.
    let matches = connection
        .prepare_cached(
            "SELECT next_sequence,last_digest,bytes_used FROM task_heads WHERE scope=?1",
        )
        .map_err(|error| sql_error(&error))?
        .query_row([key(scope)], |row| {
            let next: u32 = row.get(0)?;
            let checksum = column_bytes(row, 1, 32)?;
            let bytes = nonnegative(row, 2)?;
            Ok(next == projection.next_sequence().get() && checksum == digest.0 && bytes == used)
        })
        .optional()
        .map_err(|error| sql_error(&error))?;
    if matches == Some(true) {
        Ok(projection)
    } else {
        Err(TaskPortError::Corrupt)
    }
}

pub(crate) fn reject_retired(
    connection: &Connection,
    scope: TaskScope,
) -> Result<(), TaskPortError> {
    let retired: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM task_retirements WHERE scope=?1)",
            [key(scope)],
            |row| row.get(0),
        )
        .map_err(|error| sql_error(&error))?;
    if retired {
        Err(TaskPortError::Conflict)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "fault_tests.rs"]
mod fault_tests;

#[cfg(test)]
#[path = "key_tests.rs"]
mod key_tests;
