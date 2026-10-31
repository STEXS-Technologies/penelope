//! Public maximum-scope batch allocation budget, including caller and worker.
#[path = "../benches/support/task_read_allocator.rs"]
mod allocator;
#[path = "../benches/support/lease_claim_fixture.rs"]
mod fixture;
use fixture::{Error, Fixture};
fn main() -> Result<(), Error> {
    let selected = std::env::args().nth(1);
    let mut profiles = 0_usize;
    for timers in [false, true] {
        let name = if timers { "timer" } else { "outbox" };
        if selected.as_deref().is_some_and(|value| value != name) {
            continue;
        }
        let mut fixture = Fixture::new(timers, 128, 128)?;
        for sample in 0..10 {
            fixture.prepare(sample)?;
            let (issued, allocations, reallocations, requested_bytes) =
                allocator::measure(|| fixture.claim());
            let issued = issued?;
            fixture.verify(&issued, sample)?;
            if sample >= 2 {
                let (allocation_limit, byte_limit) = if timers {
                    (795, 184096)
                } else {
                    (1435, 324152)
                };
                if allocations > allocation_limit {
                    return Err(format!(
                        "{name}: allocations={allocations} limit={allocation_limit}"
                    )
                    .into());
                }
                if reallocations > 8 {
                    return Err(format!("{name}: reallocations={reallocations}").into());
                }
                if requested_bytes > byte_limit {
                    return Err(format!(
                        "{name}: requested_bytes={requested_bytes} limit={byte_limit}"
                    )
                    .into());
                }
            }
            if sample == 9 {
                fixture.reopen(&issued, 10)?;
            }
        }
        profiles = profiles.saturating_add(1);
    }
    if profiles == 0 {
        return Err("no selected allocation profile".into());
    }
    println!(
        "public_claim_allocation_profiles={profiles} maximum_scope=true batch=128 verified=true reopen=true"
    );
    Ok(())
}
