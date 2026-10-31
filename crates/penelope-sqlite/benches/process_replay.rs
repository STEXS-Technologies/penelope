//! Public replay latency; fixture validation stays outside the timed region.
#[path = "support/process_replay_fixture.rs"]
mod fixture;
use fixture::{Clock, Error, block_on, populate, verify};
use penelope_domain::*;
use penelope_ports::*;
use penelope_sqlite::{SqliteLimits, SqliteProcessStore};
use std::{num::NonZeroU16, sync::Arc, time::Instant};
fn main() -> Result<(), Error> {
    for count in [1_u32, 128, 4096] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("replay.db");
        let store =
            SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 10, 16)?;
        let scope = ProcessScope::new(
            "tnt_bench".parse()?,
            "prc_bench".parse()?,
            "def_bench".parse()?,
            "dfv_one".parse()?,
            ContentDigest([1; 32]),
        );
        let history = populate(&store, &scope, count)?;
        let last = count.checked_sub(1).ok_or("empty fixture")?;
        let past = count.checked_add(17).ok_or("cursor overflow")?;
        for (profile, from, limit) in [
            ("first", 0, 1),
            ("last", last, 1),
            ("end", count, 1),
            ("past", past, 1),
            ("page", 0, 128),
        ] {
            let request = OutcomeReplayRequest::new(
                scope.clone(),
                u64::from(from),
                NonZeroU16::new(limit).ok_or("zero page")?,
            )?;
            for _ in 0..100 {
                verify(
                    &block_on(store.read_outcomes(&request))??,
                    &request,
                    &history,
                )?;
            }
            let mut elapsed_ns = 0_u128;
            for _ in 0..1000 {
                let started = Instant::now();
                let page = block_on(store.read_outcomes(&request))??;
                elapsed_ns = elapsed_ns
                    .checked_add(started.elapsed().as_nanos())
                    .ok_or("duration overflow")?;
                verify(&page, &request, &history)?;
            }
            println!(
                "{}",
                serde_json::json!({"history":count,"profile":profile,"from":from,"limit":limit,"iterations":1000,"elapsed_ns":elapsed_ns})
            );
        }
        let connection = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let retained: u32 =
            connection.query_row("SELECT count(*) FROM process_outcomes", [], |row| {
                row.get(0)
            })?;
        if retained != count {
            return Err("history changed during replay".into());
        }
    }
    Ok(())
}
