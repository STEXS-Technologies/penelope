use crate::{
    SqliteTaskJournal,
    schema::{execute_one, sql_error},
    tasks::{authorized_projection, key, reject_retired},
};
use penelope_core::task::{runtime::TaskPortError, *};
use rusqlite::{TransactionBehavior, params};

/// Durable retirement frontier. Retention destroys local history bytes; callers
/// must satisfy their audit/backup obligations before explicitly requesting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRetirement {
    pub scope: TaskScope,
    pub next_sequence: TaskSequence,
    pub last_record_digest: Digest,
}

/// SQLite WAL checkpoint result. Busy readers can prevent truncation; retry
/// maintenance after releasing snapshots rather than assuming space was freed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalCheckpoint {
    pub busy: u32,
    pub wal_frames: i32,
    pub checkpointed_frames: i32,
}

impl SqliteTaskJournal {
    /// Retires one terminal history only at an exact caller-observed frontier.
    /// All reserved executions must already have confirmed termination. Keeps
    /// the retirement identity and fence tombstones so delayed starts/commits
    /// cannot resurrect the task. Never removes an individual interior record.
    /// # Errors
    /// Returns active execution, changed frontier, corruption or storage errors.
    pub fn retire_task(
        &mut self,
        scope: TaskScope,
        expected: TaskSequence,
    ) -> Result<TaskRetirement, TaskPortError> {
        self.cache = None;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error(&error))?;
        reject_retired(&transaction, scope)?;
        let projection = authorized_projection(&transaction, scope, self.definition, self.limits)?;
        if !matches!(
            projection.status(),
            TaskStatus::Completed | TaskStatus::Exhausted
        ) || projection.next_sequence() != expected
        {
            return Err(TaskPortError::Conflict);
        }
        let active: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM runtime_generations WHERE scope=?1 AND state!=2)",
                [key(scope)],
                |row| row.get(0),
            )
            .map_err(|error| sql_error(&error))?;
        if active {
            return Err(TaskPortError::Conflict);
        }
        let digest = transaction
            .query_row(
                "SELECT last_digest FROM task_heads WHERE scope=?1",
                [key(scope)],
                |row| crate::tasks::copy_column(row, 0, 32),
            )
            .map_err(|error| sql_error(&error))?;
        let digest = Digest(
            digest
                .try_into()
                .map_err(|_length| TaskPortError::Corrupt)?,
        );
        execute_one(
            &transaction,
            "INSERT INTO task_retirements VALUES (?1,?2,?3)",
            params![key(scope), expected.get(), digest.0.as_slice()],
        )?;
        for table in ["task_records", "task_blobs", "task_heads"] {
            // Table names are static adapter-owned identifiers, never input.
            transaction
                .execute(&format!("DELETE FROM {table} WHERE scope=?1"), [key(scope)])
                .map_err(|error| sql_error(&error))?;
        }
        let retained: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_records WHERE scope=?1 UNION ALL SELECT 1 FROM task_blobs WHERE scope=?1 UNION ALL SELECT 1 FROM task_heads WHERE scope=?1)",
            [key(scope)],
            |row| row.get(0),
        ).map_err(|error| sql_error(&error))?;
        if retained {
            return Err(TaskPortError::Corrupt);
        }
        transaction.commit().map_err(|error| sql_error(&error))?;
        Ok(TaskRetirement {
            scope,
            next_sequence: expected,
            last_record_digest: digest,
        })
    }

    /// Checks SQLite structural integrity. Protocol/blob verification is separate
    /// and is performed during authoritative replay or explicit head rebuilding.
    /// # Errors
    /// Returns structural corruption or unavailable storage.
    pub fn check_integrity(&self) -> Result<(), TaskPortError> {
        let mut statement = self
            .connection
            .prepare("PRAGMA quick_check(1)")
            .map_err(|error| sql_error(&error))?;
        let mut rows = statement.query([]).map_err(|error| sql_error(&error))?;
        let row = rows
            .next()
            .map_err(|error| sql_error(&error))?
            .ok_or(TaskPortError::Corrupt)?;
        if row
            .get_ref(0)
            .map_err(|error| sql_error(&error))?
            .as_str()
            .map_err(|_kind| TaskPortError::Corrupt)?
            != "ok"
        {
            return Err(TaskPortError::Corrupt);
        }
        Ok(())
    }

    /// Requests bounded WAL checkpoint/truncation during application maintenance.
    /// Deleted pages are reused; this does not shrink the main database file.
    /// # Errors
    /// Returns SQLite failures; a busy result explicitly means no truncation proof.
    pub fn checkpoint_wal(&self) -> Result<WalCheckpoint, TaskPortError> {
        self.connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok(WalCheckpoint {
                    busy: row.get(0)?,
                    wal_frames: row.get(1)?,
                    checkpointed_frames: row.get(2)?,
                })
            })
            .map_err(|error| sql_error(&error))
    }
}
