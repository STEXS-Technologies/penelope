//! Public service latency through immediate ports; setup and verification untimed.
#[path = "../tests/support/lease_batch_fixture.rs"]
mod fixture;
use std::{hint::black_box, time::Instant};
fn main() -> Result<(), fixture::Error> {
    for batch in [1, 8, 16, 32, 64, 128] {
        for width in [9, 128] {
            for order in [0, 1, 2] {
                for timers in [false, true] {
                    let fixture = fixture::Fixture::new(width, batch, order)?;
                    for _ in 0..2 {
                        let ports = fixture.prepare(timers);
                        let output = fixture.invoke(&ports, timers)?;
                        fixture.verify(&ports, &output)?;
                    }
                    let mut elapsed_ns = 0_u128;
                    let mut checksum = None;
                    for _ in 0..32 {
                        let ports = fixture.prepare(timers);
                        let began = Instant::now();
                        let output = black_box(fixture.invoke(black_box(&ports), timers)?);
                        let elapsed = began.elapsed().as_nanos();
                        elapsed_ns = elapsed_ns.checked_add(elapsed).ok_or("elapsed overflow")?;
                        let digest = fixture.verify(&ports, &output)?;
                        if checksum.is_some_and(|expected| expected != digest) {
                            return Err("unstable output".into());
                        }
                        checksum = Some(digest);
                    }
                    println!(
                        "{}",
                        serde_json::json!({"batch":batch,"width":width,"order":order,"timers":timers,"iterations":32,"elapsed_ns":elapsed_ns,"checksum":checksum})
                    );
                }
            }
        }
    }
    Ok(())
}
