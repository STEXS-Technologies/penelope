//! Transactional test fixture only; production adapters remain consumer-owned.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::missing_const_for_fn,
    clippy::map_err_ignore
)]
use penelope_core::task::{runtime::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{path::Path, time::Duration};

pub struct SqliteJournal {
    pub connection: Connection,
    pub definition: TaskDefinition,
    pub pause: Option<(&'static str, std::path::PathBuf)>,
}

fn decode(bytes: &[u8], checksum: &[u8]) -> Result<TaskRecord, TaskPortError> {
    if Digest::of(bytes).0.as_slice() != checksum {
        return Err(TaskPortError::Corrupt);
    }
    serde_json::from_slice(bytes).map_err(|_| TaskPortError::Corrupt)
}

fn pause_at(pause: &Option<(&str, std::path::PathBuf)>, boundary: &str) {
    match pause {
        Some((selected, marker)) if *selected == boundary => {
            std::fs::write(marker, boundary).unwrap();
            loop {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        _ => {}
    }
}

impl SqliteJournal {
    pub fn open(path: &Path, definition: TaskDefinition) -> Self {
        let connection = Connection::open(path).unwrap();
        connection.busy_timeout(Duration::from_secs(5)).unwrap();
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS records (
                scope BLOB NOT NULL, sequence INTEGER NOT NULL,
                record BLOB NOT NULL, checksum BLOB NOT NULL,
                PRIMARY KEY(scope, sequence));
            CREATE TABLE IF NOT EXISTS blobs (
                scope BLOB NOT NULL, digest BLOB NOT NULL, bytes BLOB NOT NULL,
                PRIMARY KEY(scope, digest));",
            )
            .unwrap();
        Self {
            connection,
            definition,
            pause: None,
        }
    }
}

impl TaskJournal for SqliteJournal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        let key = serde_json::to_vec(&scope).map_err(|_| TaskPortError::Corrupt)?;
        let stored: Option<(Vec<u8>, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT record, checksum FROM records WHERE scope=?1 AND sequence=?2",
                params![key, sequence.get()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| TaskPortError::Unavailable)?;
        stored
            .map(|(bytes, checksum)| {
                let record = decode(&bytes, &checksum)?;
                if record.scope != scope || record.sequence != sequence {
                    return Err(TaskPortError::Corrupt);
                }
                Ok(record)
            })
            .transpose()
    }

    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        let key = serde_json::to_vec(&record.scope).map_err(|_| TaskPortError::Corrupt)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| TaskPortError::Unavailable)?;
        let history = {
            let mut statement = transaction
                .prepare("SELECT record, checksum FROM records WHERE scope=?1 ORDER BY sequence")
                .map_err(|_| TaskPortError::Unavailable)?;
            let rows = statement
                .query_map(params![&key], |row| {
                    Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(|_| TaskPortError::Unavailable)?;
            let mut history = Vec::new();
            for row in rows {
                let (bytes, checksum) = row.map_err(|_| TaskPortError::Unavailable)?;
                history.push(decode(&bytes, &checksum)?);
            }
            history
        };
        let mut projection =
            replay(record.scope, self.definition, history).map_err(|_| TaskPortError::Corrupt)?;
        projection
            .apply(record)
            .map_err(|_| TaskPortError::Conflict)?;
        let reference = match record.event {
            TaskExecutionEvent::IntentCommitted { input, .. } => Some(input),
            TaskExecutionEvent::CheckpointCommitted { checkpoint }
            | TaskExecutionEvent::CheckpointMigrated { checkpoint, .. } => Some(checkpoint),
            TaskExecutionEvent::Completed { output }
            | TaskExecutionEvent::Reconciled {
                result: Reconciliation::Completed { output, .. },
            } => Some(output),
            _ => None,
        };
        match (reference, blob) {
            (Some(reference), Some(bytes))
                if reference.digest == Digest::of(bytes)
                    && usize::try_from(reference.bytes).ok() == Some(bytes.len()) =>
            {
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO blobs VALUES (?1, ?2, ?3)",
                        params![&key, reference.digest.0.as_slice(), bytes],
                    )
                    .map_err(|_| TaskPortError::Unavailable)?;
                let stored: Vec<u8> = transaction
                    .query_row(
                        "SELECT bytes FROM blobs WHERE scope=?1 AND digest=?2",
                        params![&key, reference.digest.0.as_slice()],
                        |row| row.get(0),
                    )
                    .map_err(|_| TaskPortError::Unavailable)?;
                if stored != bytes {
                    return Err(TaskPortError::Corrupt);
                }
            }
            (None, None) => {}
            _ => return Err(TaskPortError::Corrupt),
        }
        let checkpoint_commit =
            matches!(record.event, TaskExecutionEvent::CheckpointCommitted { .. });
        if checkpoint_commit {
            pause_at(&self.pause, "after-blob-insert");
        }
        let encoded = serde_json::to_vec(record).map_err(|_| TaskPortError::Corrupt)?;
        transaction
            .execute(
                "INSERT INTO records VALUES (?1, ?2, ?3, ?4)",
                params![
                    key,
                    record.sequence.get(),
                    &encoded,
                    Digest::of(&encoded).0.as_slice()
                ],
            )
            .map_err(|_| TaskPortError::Conflict)?;
        if checkpoint_commit {
            pause_at(&self.pause, "before-commit");
        }
        transaction
            .commit()
            .map_err(|_| TaskPortError::Unavailable)?;
        if checkpoint_commit {
            pause_at(&self.pause, "after-commit");
        }
        Ok(*record)
    }

    fn read_blob(
        &mut self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        let key = serde_json::to_vec(&scope).map_err(|_| TaskPortError::Corrupt)?;
        let bytes: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT bytes FROM blobs WHERE scope=?1 AND digest=?2",
                params![key, blob.digest.0.as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| TaskPortError::Unavailable)?;
        let bytes = bytes.ok_or(TaskPortError::Corrupt)?;
        if Digest::of(&bytes) != blob.digest
            || usize::try_from(blob.bytes).ok() != Some(bytes.len())
        {
            return Err(TaskPortError::Corrupt);
        }
        buffer
            .get_mut(..bytes.len())
            .ok_or(TaskPortError::Exhausted)?
            .copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}
