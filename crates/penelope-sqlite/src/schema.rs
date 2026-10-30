use penelope_core::task::runtime::TaskPortError;
use rusqlite::{Connection, TransactionBehavior};
use std::{path::Path, time::Duration};

/// Connection and admission limits. Bounds are checked before copying stored
/// records/blobs into owned memory. Database pages bound aggregate disk growth.
#[derive(Debug, Clone, Copy)]
pub struct SqliteLimits {
    pub max_blob_bytes: u32,
    pub max_record_bytes: u32,
    pub max_history_records: u32,
    pub max_scope_bytes: u64,
    pub max_scopes: u32,
    pub max_database_bytes: u64,
    pub busy_timeout: Duration,
}

impl Default for SqliteLimits {
    fn default() -> Self {
        Self {
            max_blob_bytes: 1024 * 1024,
            max_record_bytes: 16 * 1024,
            max_history_records: 65536,
            max_scope_bytes: 64 * 1024 * 1024,
            max_scopes: 1024,
            max_database_bytes: 1024 * 1024 * 1024,
            busy_timeout: Duration::from_secs(5),
        }
    }
}

// SQLite exposes non-exhaustive external error taxonomies. Unknown codes fail closed.
#[allow(clippy::wildcard_enum_match_arm)]
pub(crate) const fn sql_error(error: &rusqlite::Error) -> TaskPortError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => match failure.code {
            rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase => {
                TaskPortError::Corrupt
            }
            rusqlite::ErrorCode::DiskFull | rusqlite::ErrorCode::TooBig => TaskPortError::Exhausted,
            rusqlite::ErrorCode::ConstraintViolation => TaskPortError::Conflict,
            _ => TaskPortError::Unavailable,
        },
        _ => TaskPortError::Corrupt,
    }
}

pub(crate) fn open(path: &Path, limits: SqliteLimits) -> Result<Connection, TaskPortError> {
    if limits.max_blob_bytes == 0
        || limits.max_record_bytes < 4096
        || limits.max_history_records < 4
        || limits.max_scopes == 0
        || limits.max_scope_bytes == 0
        || limits.max_database_bytes < 65536
        || limits.busy_timeout > Duration::from_secs(60)
    {
        return Err(TaskPortError::Exhausted);
    }
    let mut connection = Connection::open(path).map_err(|error| sql_error(&error))?;
    connection
        .busy_timeout(limits.busy_timeout)
        .map_err(|error| sql_error(&error))?;
    let mode: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .map_err(|error| sql_error(&error))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(TaskPortError::Unavailable);
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|error| sql_error(&error))?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| sql_error(&error))?;
    let page_size: u32 = connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(|error| sql_error(&error))?;
    let pages = limits
        .max_database_bytes
        .checked_div(u64::from(page_size))
        .ok_or(TaskPortError::Exhausted)?;
    connection
        .pragma_update(
            None,
            "max_page_count",
            i64::try_from(pages).map_err(|_invalid| TaskPortError::Exhausted)?,
        )
        .map_err(|error| sql_error(&error))?;
    let actual_pages: u32 = connection
        .query_row("PRAGMA max_page_count", [], |row| row.get(0))
        .map_err(|error| sql_error(&error))?;
    if u64::from(actual_pages) > pages {
        return Err(TaskPortError::Exhausted);
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| sql_error(&error))?;
    let version: u32 = transaction
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| sql_error(&error))?;
    if version > 3 {
        return Err(TaskPortError::Corrupt);
    }
    if version == 0 {
        transaction.execute_batch(
            "CREATE TABLE task_heads (
                scope BLOB PRIMARY KEY CHECK(length(scope)=128),
                next_sequence INTEGER NOT NULL CHECK(next_sequence>0),
                last_digest BLOB NOT NULL CHECK(length(last_digest)=32),
                bytes_used INTEGER NOT NULL CHECK(bytes_used>=0));
             CREATE TABLE task_records (
                scope BLOB NOT NULL, sequence INTEGER NOT NULL CHECK(sequence>=0),
                record BLOB NOT NULL, checksum BLOB NOT NULL CHECK(length(checksum)=32),
                PRIMARY KEY(scope, sequence));
             CREATE TABLE task_blobs (
                scope BLOB NOT NULL, digest BLOB NOT NULL CHECK(length(digest)=32), bytes BLOB NOT NULL,
                PRIMARY KEY(scope, digest));
             CREATE TABLE runtime_generations (
                scope BLOB NOT NULL CHECK(length(scope)=128), generation INTEGER NOT NULL CHECK(generation>0),
                execution BLOB CHECK(execution IS NULL OR length(execution)=32),
                state INTEGER NOT NULL CHECK(state IN (0,1,2)),
                PRIMARY KEY(scope,generation));
             PRAGMA user_version=1;"
        ).map_err(|error| sql_error(&error))?;
    }
    if version < 2 {
        transaction
            .execute_batch(
                "CREATE TABLE task_retirements (
            scope BLOB PRIMARY KEY CHECK(length(scope)=128),
            next_sequence INTEGER NOT NULL CHECK(next_sequence>0),
            last_digest BLOB NOT NULL CHECK(length(last_digest)=32));
            PRAGMA user_version=2;",
            )
            .map_err(|error| sql_error(&error))?;
    }
    if version < 3 {
        transaction.execute_batch("CREATE TABLE process_usage (
            process_key BLOB PRIMARY KEY, bytes_used INTEGER NOT NULL CHECK(bytes_used>=0));
          CREATE TABLE process_heads (
            scope BLOB PRIMARY KEY, process_key BLOB NOT NULL UNIQUE,
            next_sequence INTEGER NOT NULL CHECK(next_sequence>0),
            bytes_used INTEGER NOT NULL CHECK(bytes_used>=0));
          CREATE TABLE process_outcomes (
            scope BLOB NOT NULL, sequence INTEGER NOT NULL CHECK(sequence>=0),
            outcome_id TEXT NOT NULL, record BLOB NOT NULL, checksum BLOB NOT NULL,
            PRIMARY KEY(scope,sequence), UNIQUE(scope,outcome_id));
          CREATE TABLE process_inbox (
            process_key BLOB NOT NULL, input_id TEXT NOT NULL,
            input BLOB NOT NULL, checksum BLOB NOT NULL, commit_digest BLOB,
            committed_through INTEGER,
            PRIMARY KEY(process_key,input_id));
          CREATE TABLE process_sources (
            process_key BLOB NOT NULL, event_id TEXT NOT NULL,
            input_id TEXT NOT NULL,
            PRIMARY KEY(process_key,event_id));
          CREATE TABLE process_commits (
            scope BLOB NOT NULL, first_sequence INTEGER NOT NULL,
            commit_digest BLOB NOT NULL, committed_through INTEGER NOT NULL,
            PRIMARY KEY(scope,first_sequence));
          CREATE TABLE process_actions (
            scope BLOB NOT NULL, action_id TEXT NOT NULL, effect_key BLOB NOT NULL,
            action BLOB NOT NULL, checksum BLOB NOT NULL,
            attempt INTEGER NOT NULL DEFAULT 0, acknowledged INTEGER NOT NULL DEFAULT 0,
            lease BLOB, expires_at INTEGER,
            PRIMARY KEY(scope,action_id), UNIQUE(scope,effect_key));
          CREATE TABLE process_timers (
            scope BLOB NOT NULL, action_id TEXT NOT NULL, timer BLOB NOT NULL,
            checksum BLOB NOT NULL, due_at INTEGER NOT NULL,
            state INTEGER NOT NULL DEFAULT 0, attempt INTEGER NOT NULL DEFAULT 0,
            lease BLOB, expires_at INTEGER,
            PRIMARY KEY(scope,action_id));
          CREATE TABLE lease_counter (singleton INTEGER PRIMARY KEY CHECK(singleton=1), next_token INTEGER NOT NULL CHECK(next_token>0));
          INSERT INTO lease_counter VALUES (1,1);
          PRAGMA user_version=3;").map_err(|error| sql_error(&error))?;
    }
    transaction.commit().map_err(|error| sql_error(&error))?;
    Ok(connection)
}
