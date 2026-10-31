//! Ordinary allocation latency for the exact private timer input implementation.
#[path = "../tests/support/timer_input_fixture.rs"]
mod fixture;
#[path = "../src/timer_input.rs"]
mod implementation;
use std::{hint::black_box, time::Instant};
fn main() -> Result<(), fixture::Error> {
    for (profile, lease) in fixture::profiles()? {
        let expected = implementation::timer_input(&lease)?;
        let checksum = penelope_domain::ContentDigest::sha256(&serde_json::to_vec(&expected)?);
        for _ in 0..100 {
            black_box(implementation::timer_input(black_box(&lease))?);
        }
        let iterations = 10000;
        let start = Instant::now();
        for _ in 0..iterations {
            black_box(implementation::timer_input(black_box(&lease))?);
        }
        let elapsed_ns = start.elapsed().as_nanos();
        println!(
            "{}",
            serde_json::json!({"profile":profile,"iterations":iterations,"elapsed_ns":elapsed_ns,"checksum":checksum,"input_id":expected.0.input_id,"payload_len":expected.1.len()})
        );
    }
    Ok(())
}
