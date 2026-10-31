//! Ordinary allocator timer validator latency; no allocation instrumentation.
#[path = "../tests/support/timer_claim_fixture.rs"]
mod fixture;
use std::{hint::black_box, time::Instant};
fn main() {
    for (profile, lease, request) in fixture::profiles() {
        let expected = lease.validate_for_claim(&request);
        for _ in 0..100 {
            let _result = black_box(black_box(&lease).validate_for_claim(black_box(&request)));
        }
        let iterations = 100000;
        let start = Instant::now();
        for _ in 0..iterations {
            let _result = black_box(black_box(&lease).validate_for_claim(black_box(&request)));
        }
        let elapsed_ns = start.elapsed().as_nanos();
        println!(
            "{}",
            serde_json::json!({"profile":profile,"iterations":iterations,"elapsed_ns":elapsed_ns,"result":format!("{expected:?}")})
        );
    }
}
