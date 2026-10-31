//! Public nonempty claims with ordinary allocation and FULL durability.
#[path = "support/lease_claim_fixture.rs"]
mod fixture;
use fixture::{Error, Fixture};
use std::time::Instant;
fn main() -> Result<(), Error> {
    for timers in [false, true] {
        for length in [9, 128] {
            for batch in [1, 16, 128] {
                let mut fixture = Fixture::new(timers, length, batch)?;
                let mut rows = Vec::new();
                for sample in 0..10 {
                    fixture.prepare(sample)?;
                    let start = Instant::now();
                    let issued = fixture.claim()?;
                    let elapsed_ns = start.elapsed().as_nanos();
                    let checksum = fixture.verify(&issued, sample)?;
                    if sample >= 2 {
                        rows.push(serde_json::json!({"timers":timers,"length":length,"batch":batch,"sample":sample,"elapsed_ns":elapsed_ns,"checksum":checksum,"verified":true}));
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
