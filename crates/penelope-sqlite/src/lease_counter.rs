//! One checked durable fencing sequence shared by independently typed protocols.
use crate::schema::sql_error;
use penelope_core::task::runtime::TaskPortError;
use rusqlite::{Connection, params};
use std::num::NonZeroU64;

pub(crate) fn next(connection: &Connection) -> Result<NonZeroU64, TaskPortError> {
    let next: i64 = connection
        .prepare_cached("SELECT next_token FROM lease_counter WHERE singleton=1")
        .map_err(|error| sql_error(&error))?
        .query_row([], |row| row.get(0))
        .map_err(|error| sql_error(&error))?;
    let value = NonZeroU64::new(u64::try_from(next).map_err(|_negative| TaskPortError::Corrupt)?)
        .ok_or(TaskPortError::Corrupt)?;
    let following = next.checked_add(1).ok_or(TaskPortError::Exhausted)?;
    let changed = connection
        .prepare_cached(
            "UPDATE lease_counter SET next_token=?1 WHERE singleton=1 AND next_token=?2",
        )
        .map_err(|error| sql_error(&error))?
        .execute(params![following, next])
        .map_err(|error| sql_error(&error))?;
    if changed != 1 {
        return Err(TaskPortError::Corrupt);
    }
    let stored: i64 = connection
        .query_row(
            "SELECT next_token FROM lease_counter WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| sql_error(&error))?;
    if stored != following {
        return Err(TaskPortError::Corrupt);
    }
    Ok(value)
}
