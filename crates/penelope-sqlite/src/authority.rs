use crate::{
    SqliteTaskJournal,
    schema::{execute_one, sql_error},
    tasks::{authorized_projection, key},
};
use penelope_core::task::{runtime::*, *};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

impl TaskGenerationAuthority for SqliteTaskJournal {
    fn reserve_start(
        &mut self,
        request: TaskStart<'_>,
        execution: Digest,
    ) -> Result<StartReservation, TaskPortError> {
        if request.definition != self.definition
            || request.scope.definition != self.definition.fingerprint()
        {
            return Err(TaskPortError::Conflict);
        }
        if request.generation == TaskGeneration::ZERO {
            return Err(TaskPortError::Conflict);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error(&error))?;
        let projection =
            authorized_projection(&transaction, request.scope, self.definition, self.limits)?;
        if projection.status() != TaskStatus::Running
            || projection.generation() != request.generation
            || projection.execution_deadline() != request.execution_deadline
        {
            return Err(TaskPortError::Conflict);
        }
        let input = projection.input().ok_or(TaskPortError::Corrupt)?;
        input
            .verify(request.input, request.input_schema)
            .map_err(|_invalid| TaskPortError::Corrupt)?;
        match (projection.checkpoint(), request.checkpoint) {
            (Some(reference), Some(bytes)) => reference
                .verify(bytes, self.definition.checkpoint_schema)
                .map_err(|_invalid| TaskPortError::Corrupt)?,
            (None, None) => {}
            _ => return Err(TaskPortError::Conflict),
        }
        let previous: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM runtime_generations WHERE scope=?1 AND generation<?2 AND state!=2)", params![key(request.scope), request.generation.get()], |row| row.get(0)).map_err(|error| sql_error(&error))?;
        if previous {
            return Err(TaskPortError::Conflict);
        }
        let stored = transaction
            .query_row(
                "SELECT execution,state FROM runtime_generations WHERE scope=?1 AND generation=?2",
                params![key(request.scope), request.generation.get()],
                |row| {
                    Ok((
                        crate::tasks::optional_digest(row, 0)?,
                        row.get::<_, u32>(1)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| sql_error(&error))?;
        let result = match stored {
            Some((id, 0)) => {
                if id != Some(execution) {
                    return Err(TaskPortError::Conflict);
                }
                StartReservation::AlreadyReserved
            }
            Some(_) => return Err(TaskPortError::Conflict),
            None => {
                execute_one(
                    &transaction,
                    "INSERT INTO runtime_generations VALUES (?1,?2,?3,0)",
                    params![
                        key(request.scope),
                        request.generation.get(),
                        execution.0.as_slice()
                    ],
                )?;
                StartReservation::Reserved
            }
        };
        transaction.commit().map_err(|error| sql_error(&error))?;
        Ok(result)
    }

    fn begin_fence(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
    ) -> Result<Option<Digest>, TaskPortError> {
        if scope.definition != self.definition.fingerprint()
            || generation == TaskGeneration::ZERO
            || generation.get() > self.definition.recovery.max_attempts
        {
            return Err(TaskPortError::Conflict);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error(&error))?;
        // A committed start (possibly not yet dispatched) is required, preventing
        // unbounded tombstones for invented scopes/generations.
        let projection = authorized_projection(&transaction, scope, self.definition, self.limits)?;
        if generation > projection.generation() {
            return Err(TaskPortError::Conflict);
        }
        execute_one(
            &transaction,
            "INSERT INTO runtime_generations VALUES (?1,?2,NULL,1) ON CONFLICT(scope,generation) DO UPDATE SET state=CASE WHEN state=2 THEN 2 ELSE 1 END",
            params![key(scope), generation.get()],
        )?;
        let id = transaction
            .query_row(
                "SELECT execution FROM runtime_generations WHERE scope=?1 AND generation=?2",
                params![key(scope), generation.get()],
                |row| crate::tasks::optional_digest(row, 0),
            )
            .map_err(|error| sql_error(&error))?;
        let result = id;
        transaction.commit().map_err(|error| sql_error(&error))?;
        Ok(result)
    }

    fn confirm_stopped(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        execution: Option<Digest>,
    ) -> Result<(), TaskPortError> {
        if scope.definition != self.definition.fingerprint() {
            return Err(TaskPortError::Conflict);
        }
        let changed = self.connection.execute("UPDATE runtime_generations SET state=2 WHERE scope=?1 AND generation=?2 AND execution IS ?3 AND state IN (1,2)", params![key(scope), generation.get(), execution.as_ref().map(|id| id.0.as_slice())]).map_err(|error| sql_error(&error))?;
        if changed != 1 {
            return Err(TaskPortError::Conflict);
        }
        Ok(())
    }
}
