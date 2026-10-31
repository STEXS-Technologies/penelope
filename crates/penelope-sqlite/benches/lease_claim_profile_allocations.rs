//! Public claim allocation traffic; never used for latency measurement.
#[path = "support/task_read_allocator.rs"]
mod allocator;
#[path = "support/lease_claim_fixture.rs"]
mod fixture;
use fixture::{Error, Fixture};
fn main() -> Result<(), Error> {
    for timers in [false, true] {
        for length in [9, 128] {
            for batch in [1, 16, 128] {
                let mut fixture = Fixture::new(timers, length, batch)?;
                let mut rows = Vec::new();
                for sample in 0..10 {
                    fixture.prepare(sample)?;
                    let (issued, allocations, reallocations, requested_bytes) =
                        allocator::measure(|| fixture.claim());
                    let issued = issued?;
                    let checksum = fixture.verify(&issued, sample)?;
                    if sample >= 2 {
                        rows.push(serde_json::json!({"timers":timers,"length":length,"batch":batch,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"checksum":checksum,"verified":true}));
                    }
                    if sample == 9 {
                        fixture.reopen(&issued, 10)?;
                    }
                }
                for row in rows {
                    println!("{row}");
                }
            }
        }
    }
    Ok(())
}
