//! SQLite-engine selection work with synthetic retained rows; no public claim timing.
use penelope_domain::LogicalTime;
use penelope_ports::PortError;
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore};
use rusqlite::{Connection, StatementStatus, params};
use serde_json::json;
use std::sync::Arc;
type Error = Box<dyn std::error::Error>;
const OUTBOX: &str = "SELECT action,checksum,attempt,action_id,effect_key FROM process_actions WHERE scope=?1 AND acknowledged=0 AND (expires_at IS NULL OR expires_at<?2) ORDER BY action_id LIMIT ?3";
const TIMERS: &str = "SELECT timer,checksum,attempt,action_id,due_at FROM process_timers WHERE scope=?1 AND state=0 AND due_at<=?2 AND (expires_at IS NULL OR expires_at<?2) ORDER BY due_at,action_id LIMIT ?3";
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
fn work(connection: &Connection, sql: &str) -> Result<serde_json::Value, Error> {
    let plan = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?
        .query_map(params![b"scope".as_slice(), 100, 16], |row| {
            row.get::<_, String>(3)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut statement = connection.prepare(sql)?;
    let mut identities = Vec::new();
    {
        let mut rows = statement.query(params![b"scope".as_slice(), 100, 16])?;
        while let Some(row) = rows.next()? {
            identities.push(row.get::<_, String>(3)?);
        }
    }
    if identities.len() != 16 {
        return Err("wrong selected count".into());
    }
    for (index, identity) in identities.iter().enumerate() {
        if identity != &format!("pending_{index:03}") {
            return Err("wrong selected order".into());
        }
    }
    Ok(
        json!({"vm_steps":statement.get_status(StatementStatus::VmStep),"sorts":statement.get_status(StatementStatus::Sort),"plan":plan,"identities":identities}),
    )
}
fn mutation(connection: &Connection, sql: &str) -> Result<i32, Error> {
    let mut statement = connection.prepare(sql)?;
    if statement.execute([])? != 1 {
        return Err("wrong mutation count".into());
    }
    Ok(statement.get_status(StatementStatus::VmStep))
}
fn main() -> Result<(), Error> {
    for retained in [0_u32, 1024, 16384] {
        for future in [false, true] {
            for indexed in [false, true] {
                let directory = tempfile::tempdir()?;
                let path = directory.path().join("selection.db");
                let store = SqliteProcessStore::open(
                    &path,
                    SqliteLimits::default(),
                    Arc::new(Clock),
                    10,
                    1,
                )?;
                let mut connection = Connection::open(&path)?;
                connection.execute_batch("DROP INDEX IF EXISTS process_actions_pending; DROP INDEX IF EXISTS process_timers_due;")?;
                if indexed {
                    connection.execute_batch("CREATE INDEX process_actions_pending ON process_actions(scope,action_id) WHERE acknowledged=0; CREATE INDEX process_timers_due ON process_timers(scope,due_at,action_id) WHERE state=0;")?;
                }
                let transaction = connection.transaction()?;
                // Opaque values intentionally exercise only the SQL engine. They
                // are not decoded as domain actions or passed to public claims.
                transaction.execute("WITH RECURSIVE ids(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM ids WHERE n+1<?1) INSERT INTO process_actions(scope,action_id,effect_key,action,checksum,acknowledged) SELECT ?2,printf('history_%06d',n),CAST(printf('history_%06d',n) AS BLOB),zeroblob(256),zeroblob(32),1 FROM ids WHERE ?1>0",params![retained,b"scope".as_slice()])?;
                transaction.execute("INSERT INTO process_timers(scope,action_id,timer,checksum,due_at,state) SELECT scope,action_id,action,checksum,?1,?2 FROM process_actions",params![if future {200} else {1},if future {0} else {1}])?;
                transaction.execute("WITH RECURSIVE ids(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM ids WHERE n<15) INSERT INTO process_actions(scope,action_id,effect_key,action,checksum) SELECT ?1,printf('pending_%03d',n),CAST(printf('pending_%03d',n) AS BLOB),zeroblob(256),zeroblob(32) FROM ids",[b"scope".as_slice()])?;
                transaction.execute("INSERT INTO process_timers(scope,action_id,timer,checksum,due_at) SELECT scope,action_id,action,checksum,100 FROM process_actions WHERE acknowledged=0",[])?;
                transaction.commit()?;
                let outbox = work(&connection, OUTBOX)?;
                let timers = work(&connection, TIMERS)?;
                let pages: u32 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
                let writes = json!({
                    "outbox_insert": mutation(&connection,"INSERT INTO process_actions(scope,action_id,effect_key,action,checksum) VALUES (X'73636f7065','write_one',X'7772697465',zeroblob(256),zeroblob(32))")?,
                    "outbox_lease_update": mutation(&connection,"UPDATE process_actions SET attempt=attempt+1,lease=zeroblob(256),expires_at=200 WHERE scope=X'73636f7065' AND action_id='write_one'")?,
                    "outbox_ack": mutation(&connection,"UPDATE process_actions SET acknowledged=1 WHERE scope=X'73636f7065' AND action_id='write_one'")?,
                    "timer_insert": mutation(&connection,"INSERT INTO process_timers(scope,action_id,timer,checksum,due_at) VALUES (X'73636f7065','write_one',zeroblob(256),zeroblob(32),100)")?,
                    "timer_lease_update": mutation(&connection,"UPDATE process_timers SET attempt=attempt+1,lease=zeroblob(256),expires_at=200 WHERE scope=X'73636f7065' AND action_id='write_one'")?,
                    "timer_ack": mutation(&connection,"UPDATE process_timers SET state=1 WHERE scope=X'73636f7065' AND action_id='write_one'")?,
                });
                println!(
                    "{}",
                    json!({"retained":retained,"future_timers":future,"indexed":indexed,"outbox":outbox,"timers":timers,"database_pages":pages,"mutation_vm_steps":writes,"sqlite_version":rusqlite::version()})
                );
                drop(store);
            }
        }
    }
    Ok(())
}
