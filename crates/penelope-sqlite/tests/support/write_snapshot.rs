//! Logical row snapshots for transaction rollback fault tests.
use rusqlite::{Connection, types::Value};
pub(super) type Snapshot = Vec<(String, Vec<Vec<Value>>)>;
pub(super) fn snapshot(connection: &Connection, tables: &[&str]) -> Snapshot {
    tables
        .iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|column| row.get::<_, Value>(column))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            ((*table).to_owned(), rows)
        })
        .collect()
}
