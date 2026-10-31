//! Caller-thread first-poll allocation traffic; no latency or worker-allocation claim.
#[path = "support/caller_allocator.rs"]
mod allocator;
#[path = "support/process_append_fixture.rs"]
mod fixture;
use fixture::{Clock, Error, Fixture, block_on};
use penelope_ports::ProcessStore;
use penelope_sqlite::{SqliteLimits, SqliteProcessStore};
use std::{sync::Arc, task::Context};
fn main() -> Result<(), Error> {
    for scope_len in [9, 128] {
        for id_len in [8, 128] {
            for count in [1, 128] {
                let fixture = Fixture::new(scope_len, id_len, count)?;
                let directory = tempfile::tempdir()?;
                let path = directory.path().join("append.db");
                let store = SqliteProcessStore::open(
                    &path,
                    SqliteLimits::default(),
                    Arc::new(Clock),
                    10,
                    16,
                )?;
                block_on(store.append_outcomes(&fixture.scope, 0, &fixture.outcomes))??;
                fixture.verify(&store)?;
                let connection = rusqlite::Connection::open(&path)?;
                for sample in 0..8 {
                    connection.execute_batch("BEGIN IMMEDIATE")?;
                    let waker = fixture::waker();
                    let mut context = Context::from_waker(&waker);
                    let mut future = store.append_outcomes(&fixture.scope, 0, &fixture.outcomes);
                    let (state, allocations, reallocations, requested_bytes) =
                        allocator::measure(|| future.as_mut().poll(&mut context));
                    if state.is_ready() {
                        return Err("write gate did not hold the admitted append".into());
                    }
                    connection.execute_batch("COMMIT")?;
                    block_on(future)??;
                    fixture.verify(&store)?;
                    println!(
                        "{}",
                        serde_json::json!({"scope_len":scope_len,"id_len":id_len,"count":count,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"checksum":fixture.checksum,"verified":true})
                    );
                }
            }
        }
    }
    Ok(())
}
