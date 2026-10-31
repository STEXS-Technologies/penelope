//! Rejection must avoid owned scope copies; admission needs only one owned batch.
#[path = "../benches/support/caller_allocator.rs"]
mod allocator;
#[path = "../benches/support/process_append_fixture.rs"]
mod fixture;
use fixture::{Clock, Error, Fixture, block_on};
use penelope_domain::ContentDigest;
use penelope_ports::{PortError, ProcessStore};
use penelope_sqlite::{SqliteLimits, SqliteProcessStore};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
fn main() -> Result<(), Error> {
    let fixture = Fixture::new(128, 128, 128)?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("guard.db");
    let store = SqliteProcessStore::open(&path, SqliteLimits::default(), Arc::new(Clock), 10, 16)?;
    let mut wrong = fixture.outcomes.clone();
    wrong.last_mut().ok_or("empty fixture")?.definition_digest = ContentDigest([8; 32]);
    let waker = fixture::waker();
    let mut context = Context::from_waker(&waker);
    let mut rejected = store.append_outcomes(&fixture.scope, 0, &wrong);
    let (rejection_state, rejection_allocations, rejection_reallocations, _) =
        allocator::measure(|| rejected.as_mut().poll(&mut context));
    if rejection_state != Poll::Ready(Err(PortError::Invariant))
        || rejection_allocations != 0
        || rejection_reallocations != 0
    {
        return Err(format!("scope rejection cloned strings: rejection_state={rejection_state:?} rejection_allocations={rejection_allocations} rejection_reallocations={rejection_reallocations}").into());
    }
    drop(rejected);
    block_on(store.append_outcomes(&fixture.scope, 0, &fixture.outcomes))??;
    fixture.verify(&store)?;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let mut accepted = store.append_outcomes(&fixture.scope, 0, &fixture.outcomes);
    let (state, allocations, reallocations, _) =
        allocator::measure(|| accepted.as_mut().poll(&mut context));
    // Six heap identifiers per outcome; one Vec, four validation scope strings,
    // completion state and job box, plus one allocation of budget headroom.
    let one_batch_budget = fixture
        .outcomes
        .len()
        .checked_mul(6)
        .and_then(|count| count.checked_add(8))
        .ok_or("allocation budget overflow")?;
    if !state.is_pending() || allocations > one_batch_budget || reallocations != 0 {
        return Err(format!("append admission copied more than one batch: state={state:?} allocations={allocations} reallocations={reallocations}").into());
    }
    connection.execute_batch("COMMIT")?;
    block_on(accepted)??;
    fixture.verify(&store)?;
    println!(
        "scope_rejection_allocations=0 repaired=true admission_allocations={allocations} single_batch=true checksum={:?}",
        fixture.checksum
    );
    Ok(())
}
