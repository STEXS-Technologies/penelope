use crate::{
    SqliteLimits,
    schema::{execute_one, open, sql_error},
    tasks::copy_column,
    worker::DatabaseWorker,
};
use async_trait::async_trait;
use penelope_core::task::{Digest, runtime::TaskPortError};
use penelope_domain::{LogicalTime, ProcessInput, ProcessOutcome, ProcessScope};
use penelope_ports::*;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{path::Path, sync::Arc};

/// Trusted synchronous clock evaluated on the database worker. Units are saga
/// logical milliseconds, never task ticks. Must not move backwards across reopen.
pub trait SqliteLeaseClock: Send + Sync {
    /// # Errors
    /// Returns unavailable or invalid trusted time.
    fn now(&self) -> Result<LogicalTime, PortError>;
}

/// Optional async saga adapter. All database work executes on a bounded worker
/// queue, so awaiting these ports never performs SQLite I/O on a reactor thread.
pub struct SqliteProcessStore {
    pub(crate) worker: DatabaseWorker,
    pub(crate) limits: SqliteLimits,
    pub(crate) clock: Arc<dyn SqliteLeaseClock>,
    pub(crate) lease_millis: u64,
}

pub(crate) const fn port_error(error: TaskPortError) -> PortError {
    match error {
        TaskPortError::Unavailable | TaskPortError::Runtime => PortError::Unavailable,
        TaskPortError::Exhausted => PortError::QuotaExceeded,
        TaskPortError::Corrupt => PortError::Invariant,
        TaskPortError::Conflict => PortError::Conflict,
    }
}
pub(crate) const fn db_error(error: &rusqlite::Error) -> PortError {
    port_error(sql_error(error))
}
pub(crate) fn integer(value: u64) -> Result<i64, PortError> {
    i64::try_from(value).map_err(|_overflow| PortError::QuotaExceeded)
}
pub(crate) fn encode<T: Serialize>(value: &T, limit: u32) -> Result<Vec<u8>, PortError> {
    let bytes = serde_json::to_vec(value).map_err(|_codec| PortError::Invariant)?;
    if u32::try_from(bytes.len())
        .ok()
        .is_none_or(|size| size > limit)
    {
        return Err(PortError::QuotaExceeded);
    }
    Ok(bytes)
}
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8], checksum: &[u8]) -> Result<T, PortError> {
    if Digest::of(bytes).0.as_slice() != checksum {
        return Err(PortError::Invariant);
    }
    serde_json::from_slice(bytes).map_err(|_codec| PortError::Invariant)
}
pub(crate) fn process_key(input: &ProcessInput) -> Result<Vec<u8>, PortError> {
    encode(&(&input.tenant_id, &input.process_id), 1024)
}
pub(crate) fn scope_process_key(scope: &ProcessScope) -> Result<Vec<u8>, PortError> {
    encode(&(&scope.tenant_id, &scope.process_id), 1024)
}

impl SqliteProcessStore {
    /// Opens/migrates before creating the blocking-isolation thread. Invoke
    /// construction outside a latency-sensitive async reactor.
    /// # Errors
    /// Returns unsupported storage, invalid bounds or worker creation failures.
    pub fn open(
        path: &Path,
        limits: SqliteLimits,
        clock: Arc<dyn SqliteLeaseClock>,
        lease_millis: u64,
        queue_capacity: u16,
    ) -> Result<Self, PortError> {
        if lease_millis == 0 {
            return Err(PortError::Invariant);
        }
        integer(lease_millis)?;
        let connection = open(path, limits).map_err(port_error)?;
        Ok(Self {
            worker: DatabaseWorker::new(connection, queue_capacity)?,
            limits,
            clock,
            lease_millis,
        })
    }

    /// Returns the bound on requests queued behind the active operation.
    #[must_use]
    pub const fn queue_capacity(&self) -> u16 {
        self.worker.capacity()
    }
}

fn accept_input(
    connection: &Connection,
    input: &ProcessInput,
    limits: SqliteLimits,
) -> Result<bool, PortError> {
    input.validate().map_err(|_invalid| PortError::Invariant)?;
    let bytes = encode(input, limits.max_record_bytes)?;
    let key = process_key(input)?;
    let stored = connection
        .query_row(
            "SELECT input,checksum FROM process_inbox WHERE process_key=?1 AND input_id=?2",
            params![key, input.input_id.as_str()],
            |row| {
                Ok((
                    copy_column(row, 0, limits.max_record_bytes)?,
                    copy_column(row, 1, 32)?,
                ))
            },
        )
        .optional()
        .map_err(|error| db_error(&error))?;
    if let Some((stored, checksum)) = stored {
        let decoded: ProcessInput = decode(&stored, &checksum)?;
        if decoded != *input {
            return Err(PortError::Conflict);
        }
        return Ok(true);
    }
    let count: u32 = connection
        .query_row(
            "SELECT count(*) FROM process_inbox WHERE process_key=?1",
            [&key],
            |row| row.get(0),
        )
        .map_err(|error| db_error(&error))?;
    if count >= limits.max_history_records {
        return Err(PortError::QuotaExceeded);
    }
    charge_process(
        connection,
        &key,
        u64::try_from(bytes.len())
            .map_err(|_overflow| PortError::QuotaExceeded)?
            .saturating_add(128),
        limits,
    )?;
    execute_one(
        connection,
        "INSERT INTO process_inbox(process_key,input_id,input,checksum) VALUES (?1,?2,?3,?4)",
        params![
            key,
            input.input_id.as_str(),
            bytes,
            Digest::of(&bytes).0.as_slice()
        ],
    )
    .map_err(port_error)?;
    Ok(false)
}

pub(crate) fn commit_process(
    connection: &mut Connection,
    commit: &AtomicProcessCommit,
    limits: SqliteLimits,
) -> Result<AtomicProcessCommitReceipt, PortError> {
    commit.validate().map_err(|_invalid| PortError::Invariant)?;
    let scope = commit.outcomes.first().ok_or(PortError::Invariant)?.scope();
    let key = scope.canonical_bytes();
    let process_key = scope_process_key(&scope)?;
    // Cap the aggregate encoding allowance instead of rejecting overflow: a
    // larger per-record limit must still admit an identical small batch.
    let max_encoded = limits.max_record_bytes.saturating_mul(
        u32::try_from(
            MAX_OUTCOMES_PER_ATOMIC_COMMIT
                .saturating_add(MAX_ACTIONS_PER_ATOMIC_COMMIT)
                .saturating_add(2),
        )
        .map_err(|_overflow| PortError::QuotaExceeded)?,
    );
    let commit_bytes = encode(commit, max_encoded)?;
    let commit_digest = Digest::of(&commit_bytes);
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| db_error(&error))?;
    let existing = transaction.query_row("SELECT commit_digest,committed_through FROM process_commits WHERE scope=?1 AND first_sequence=?2", params![key,integer(commit.expected_sequence)?], |row| Ok((copy_column(row,0,32)?,row.get::<_,i64>(1)?))).optional().map_err(|error| db_error(&error))?;
    if let Some((digest, through)) = existing {
        if digest != commit_digest.0 {
            return Err(PortError::Conflict);
        }
        verify_committed_batch(&transaction, commit, commit_digest, limits)?;
        let receipt = AtomicProcessCommitReceipt {
            committed_through_sequence: u64::try_from(through)
                .map_err(|_negative| PortError::Invariant)?,
            duplicate_input: commit.input.is_some(),
        };
        receipt
            .validate_for(commit)
            .map_err(|_receipt| PortError::Invariant)?;
        return Ok(receipt);
    }
    let head = transaction
        .query_row(
            "SELECT next_sequence,bytes_used FROM process_heads WHERE scope=?1",
            [&key],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| db_error(&error))?;
    let (next, used) = head.unwrap_or((0, 0));
    if integer(commit.expected_sequence)? != next {
        return Err(PortError::Conflict);
    }
    let mut duplicate_input = false;
    if let Some(input) = &commit.input {
        duplicate_input = accept_input(&transaction, input, limits)?;
        let consumed: bool = transaction.query_row("SELECT commit_digest IS NOT NULL FROM process_inbox WHERE process_key=?1 AND input_id=?2", params![process_key,input.input_id.as_str()], |row| row.get(0)).map_err(|error| db_error(&error))?;
        if consumed {
            return Err(PortError::Conflict);
        }
        if let Some(source) = &commit.canonical_source_event_id {
            execute_one(
                &transaction,
                "INSERT INTO process_sources VALUES (?1,?2,?3)",
                params![process_key, source.as_str(), input.input_id.as_str()],
            )
            .map_err(port_error)?;
        }
    }
    let new_next = commit
        .expected_sequence
        .checked_add(
            u64::try_from(commit.outcomes.len()).map_err(|_overflow| PortError::QuotaExceeded)?,
        )
        .ok_or(PortError::QuotaExceeded)?;
    if new_next > u64::from(limits.max_history_records)
        || new_next > MAX_OUTCOMES_PER_REPLAY_LOG as u64
    {
        return Err(PortError::QuotaExceeded);
    }
    let mut added = 0_u64;
    for outcome in &commit.outcomes {
        let bytes = encode(outcome, limits.max_record_bytes)?;
        added = added
            .checked_add(u64::try_from(bytes.len()).map_err(|_overflow| PortError::QuotaExceeded)?)
            .ok_or(PortError::QuotaExceeded)?;
        execute_one(
            &transaction,
            "INSERT INTO process_outcomes VALUES (?1,?2,?3,?4,?5)",
            params![
                key,
                integer(outcome.sequence)?,
                outcome.outcome_id.as_str(),
                bytes,
                Digest::of(&bytes).0.as_slice()
            ],
        )
        .map_err(port_error)?;
    }
    for action in &commit.actions {
        let bytes = encode(action, limits.max_record_bytes)?;
        added = added
            .checked_add(u64::try_from(bytes.len()).map_err(|_overflow| PortError::QuotaExceeded)?)
            .ok_or(PortError::QuotaExceeded)?;
        execute_one(&transaction,"INSERT INTO process_actions(scope,action_id,effect_key,action,checksum) VALUES (?1,?2,?3,?4,?5)", params![key,action.action_id.as_str(),action.effect_key().canonical_bytes(),bytes,Digest::of(&bytes).0.as_slice()]).map_err(port_error)?;
    }
    let total = u64::try_from(used)
        .map_err(|_negative| PortError::Invariant)?
        .checked_add(added)
        .ok_or(PortError::QuotaExceeded)?;
    if total > limits.max_scope_bytes {
        return Err(PortError::QuotaExceeded);
    }
    charge_process(
        &transaction,
        &process_key,
        added.saturating_add(128),
        limits,
    )?;
    if head.is_none() {
        let count: u32 = transaction
            .query_row("SELECT count(*) FROM process_heads", [], |row| row.get(0))
            .map_err(|error| db_error(&error))?;
        if count >= limits.max_scopes {
            return Err(PortError::QuotaExceeded);
        }
        execute_one(
            &transaction,
            "INSERT INTO process_heads VALUES (?1,?2,?3,?4)",
            params![key, process_key, integer(new_next)?, integer(total)?],
        )
        .map_err(port_error)?;
    } else {
        let changed = transaction.execute("UPDATE process_heads SET next_sequence=?1,bytes_used=?2 WHERE scope=?3 AND next_sequence=?4", params![integer(new_next)?,integer(total)?,key,next]).map_err(|error| db_error(&error))?;
        if changed != 1 {
            return Err(PortError::Conflict);
        }
    }
    let through = new_next.checked_sub(1).ok_or(PortError::Invariant)?;
    execute_one(
        &transaction,
        "INSERT INTO process_commits VALUES (?1,?2,?3,?4)",
        params![
            key,
            integer(commit.expected_sequence)?,
            commit_digest.0.as_slice(),
            integer(through)?
        ],
    )
    .map_err(port_error)?;
    if let Some(input) = &commit.input {
        execute_one(&transaction,"UPDATE process_inbox SET commit_digest=?1,committed_through=?2 WHERE process_key=?3 AND input_id=?4", params![commit_digest.0.as_slice(),integer(through)?,process_key,input.input_id.as_str()]).map_err(port_error)?;
    }
    let receipt = AtomicProcessCommitReceipt {
        committed_through_sequence: through,
        duplicate_input,
    };
    receipt
        .validate_for(commit)
        .map_err(|_receipt| PortError::Invariant)?;
    transaction.commit().map_err(|error| db_error(&error))?;
    Ok(receipt)
}

#[async_trait]
impl ProcessStore for SqliteProcessStore {
    async fn commit(
        &self,
        commit: &AtomicProcessCommit,
    ) -> Result<AtomicProcessCommitReceipt, PortError> {
        commit.validate().map_err(|_invalid| PortError::Invariant)?;
        let commit = commit.clone();
        let limits = self.limits;
        self.worker
            .run(move |connection| commit_process(connection, &commit, limits))?
            .await
    }
    async fn append_outcomes(
        &self,
        scope: &ProcessScope,
        expected_sequence: u64,
        outcomes: &[ProcessOutcome],
    ) -> Result<(), PortError> {
        if outcomes.len() > MAX_OUTCOMES_PER_ATOMIC_COMMIT {
            return Err(PortError::QuotaExceeded);
        }
        if outcomes.iter().any(|outcome| outcome.scope() != *scope) {
            return Err(PortError::Invariant);
        }
        let commit =
            AtomicProcessCommit::new(expected_sequence, None, outcomes.to_vec(), Vec::new())
                .map_err(|_invalid| PortError::Invariant)?;
        ProcessStore::commit(self, &commit).await.map(|_receipt| ())
    }
    async fn read_outcomes(
        &self,
        request: &OutcomeReplayRequest,
    ) -> Result<OutcomeReplayPage, PortError> {
        request
            .validate()
            .map_err(|_invalid| PortError::Invariant)?;
        let request = request.clone();
        let limits = self.limits;
        self.worker.run(move |connection| {
            let transaction = connection.transaction().map_err(|error| db_error(&error))?;
            let key = request.scope.canonical_bytes();
            let next: Option<i64> = transaction.query_row("SELECT next_sequence FROM process_heads WHERE scope=?1", [&key], |row| row.get(0)).optional().map_err(|error| db_error(&error))?;
            let next = u64::try_from(next.unwrap_or(0)).map_err(|_negative| PortError::Invariant)?;
            let mut statement = transaction.prepare("SELECT sequence,record,checksum FROM process_outcomes WHERE scope=?1 AND sequence>=?2 ORDER BY sequence LIMIT ?3").map_err(|error| db_error(&error))?;
            let mut rows = statement.query(params![key,integer(request.from_sequence)?,request.limit.get()]).map_err(|error| db_error(&error))?;
            let mut outcomes = Vec::new(); let mut expected = request.from_sequence;
            while let Some(row) = rows.next().map_err(|error| db_error(&error))? {
                let sequence: i64 = row.get(0).map_err(|error| db_error(&error))?;
                let bytes = copy_column(row,1,limits.max_record_bytes).map_err(|error| db_error(&error))?; let checksum = copy_column(row,2,32).map_err(|error| db_error(&error))?;
                let outcome: ProcessOutcome = decode(&bytes,&checksum)?;
                if sequence != integer(expected)? || outcome.sequence != expected || outcome.scope() != request.scope || outcome.validate().is_err() || expected >= next { return Err(PortError::Invariant); }
                expected = expected.checked_add(1).ok_or(PortError::Invariant)?;
                outcomes.push(outcome);
            }
            let required = next.saturating_sub(request.from_sequence).min(u64::from(request.limit.get()));
            if u64::try_from(outcomes.len()).ok() != Some(required) { return Err(PortError::Invariant); }
            OutcomeReplayPage::new(request.scope,request.from_sequence,outcomes, if expected < next { Some(expected) } else { None }).map_err(|_invalid| PortError::Invariant)
        })?.await
    }
}

#[async_trait]
impl Inbox for SqliteProcessStore {
    async fn accept(&self, input: &ProcessInput) -> Result<InboxAcceptanceReceipt, PortError> {
        input.validate().map_err(|_invalid| PortError::Invariant)?;
        let input = input.clone();
        let limits = self.limits;
        self.worker
            .run(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|error| db_error(&error))?;
                let duplicate = accept_input(&transaction, &input, limits)?;
                transaction.commit().map_err(|error| db_error(&error))?;
                Ok(InboxAcceptanceReceipt {
                    tenant_id: input.tenant_id,
                    process_id: input.process_id,
                    input_id: input.input_id,
                    duplicate,
                })
            })?
            .await
    }
}

pub(crate) fn charge_process(
    connection: &Connection,
    key: &[u8],
    added: u64,
    limits: SqliteLimits,
) -> Result<(), PortError> {
    let used: Option<i64> = connection
        .query_row(
            "SELECT bytes_used FROM process_usage WHERE process_key=?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| db_error(&error))?;
    let next = u64::try_from(used.unwrap_or(0))
        .map_err(|_negative| PortError::Invariant)?
        .checked_add(added)
        .ok_or(PortError::QuotaExceeded)?;
    if next > limits.max_scope_bytes {
        return Err(PortError::QuotaExceeded);
    }
    if used.is_none() {
        let count: u32 = connection
            .query_row("SELECT count(*) FROM process_usage", [], |row| row.get(0))
            .map_err(|error| db_error(&error))?;
        if count >= limits.max_scopes {
            return Err(PortError::QuotaExceeded);
        }
        execute_one(
            connection,
            "INSERT INTO process_usage VALUES (?1,?2)",
            params![key, integer(next)?],
        )
        .map_err(port_error)?;
    } else {
        execute_one(
            connection,
            "UPDATE process_usage SET bytes_used=?1 WHERE process_key=?2",
            params![integer(next)?, key],
        )
        .map_err(port_error)?;
    }
    Ok(())
}

fn verify_committed_batch(
    connection: &Connection,
    commit: &AtomicProcessCommit,
    commit_digest: Digest,
    limits: SqliteLimits,
) -> Result<(), PortError> {
    let scope = commit.outcomes.first().ok_or(PortError::Invariant)?.scope();
    let key = scope.canonical_bytes();
    let frontier: Option<i64> = connection
        .query_row(
            "SELECT next_sequence FROM process_heads WHERE scope=?1",
            [&key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| db_error(&error))?;
    let expected_end = commit
        .expected_sequence
        .checked_add(
            u64::try_from(commit.outcomes.len()).map_err(|_overflow| PortError::Invariant)?,
        )
        .ok_or(PortError::Invariant)?;
    let expected_end = integer(expected_end)?;
    if frontier.is_none_or(|next| next < expected_end) {
        return Err(PortError::Invariant);
    }
    for outcome in &commit.outcomes {
        let (bytes, checksum) = connection
            .query_row(
                "SELECT record,checksum FROM process_outcomes WHERE scope=?1 AND sequence=?2",
                params![key, integer(outcome.sequence)?],
                |row| {
                    Ok((
                        copy_column(row, 0, limits.max_record_bytes)?,
                        copy_column(row, 1, 32)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| db_error(&error))?
            .ok_or(PortError::Invariant)?;
        if decode::<ProcessOutcome>(&bytes, &checksum)? != *outcome {
            return Err(PortError::Invariant);
        }
    }
    for action in &commit.actions {
        let (bytes, checksum) = connection
            .query_row(
                "SELECT action,checksum FROM process_actions WHERE scope=?1 AND action_id=?2 AND effect_key=?3",
                params![key, action.action_id.as_str(), action.effect_key().canonical_bytes()],
                |row| {
                    Ok((
                        copy_column(row, 0, limits.max_record_bytes)?,
                        copy_column(row, 1, 32)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| db_error(&error))?
            .ok_or(PortError::Invariant)?;
        if decode::<penelope_domain::ProcessAction>(&bytes, &checksum)? != *action {
            return Err(PortError::Invariant);
        }
    }
    if let Some(input) = &commit.input {
        // Consumption is part of the committed batch: an arbitrary non-null
        // marker cannot prove this input belongs to this exact receipt.
        let (bytes, checksum) = connection
            .query_row(
                "SELECT input,checksum FROM process_inbox WHERE process_key=?1 AND input_id=?2 AND commit_digest=?3 AND committed_through=?4",
                params![process_key(input)?, input.input_id.as_str(), commit_digest.0.as_slice(), expected_end.checked_sub(1).ok_or(PortError::Invariant)?],
                |row| Ok((copy_column(row, 0, limits.max_record_bytes)?, copy_column(row, 1, 32)?)),
            )
            .optional()
            .map_err(|error| db_error(&error))?
            .ok_or(PortError::Invariant)?;
        if decode::<ProcessInput>(&bytes, &checksum)? != *input {
            return Err(PortError::Invariant);
        }
        if let Some(source) = &commit.canonical_source_event_id {
            let bound: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM process_sources WHERE process_key=?1 AND event_id=?2 AND input_id=?3)",
                params![process_key(input)?, source.as_str(), input.input_id.as_str()],
                |row| row.get(0),
            ).map_err(|error| db_error(&error))?;
            if !bound {
                return Err(PortError::Invariant);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "process_fault_tests.rs"]
mod fault_tests;
