//! Public validator CPU latency, with the ordinary system allocator.
#![allow(clippy::unwrap_used)]
use std::{hint::black_box, time::Instant};
#[path = "../tests/support/atomic_validation_fixture.rs"]
mod fixture;
fn main() {
    fixture::commit(0).validate().unwrap();
    let long = std::env::var_os("PENELOPE_BENCH_LONG_IDS").is_some();
    for (actions, iterations) in [(0, 100_000_u32), (1, 100_000), (16, 10_000), (128, 1_000)] {
        let iterations = if long { iterations / 10 } else { iterations };
        let batch = fixture::commit_with_identifiers(actions, long);
        for _ in 0..100 {
            black_box(black_box(&batch).validate()).unwrap();
        }
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(black_box(&batch).validate()).unwrap();
        }
        let elapsed_ns = started.elapsed().as_nanos();
        println!("actions={actions} iterations={iterations} elapsed_ns={elapsed_ns}");
    }
}
