//! Shared hosted sampler. Formatting and allocation probes are outside timing; callback checks remain included.
#[path = "../../tests/support/allocations.rs"]
mod allocations;
use allocations::{Counts, start_counting, stop_counting};
use std::{hint::black_box, time::Instant};
pub(super) const SAMPLES: usize = 101;
pub(super) const BATCH: usize = 32;
pub(super) struct Measurements(Vec<u128>);
pub(super) fn positive_control() {
    start_counting();
    let control = black_box(Vec::<u8>::with_capacity(16));
    drop(black_box(control));
    assert_eq!(
        stop_counting(),
        Counts {
            allocations: 1,
            reallocations: 0,
            deallocations: 1,
            requested_bytes: 16
        }
    );
}
pub(super) fn measure(mut operation: impl FnMut(u64)) -> Measurements {
    for step in 0..128 {
        operation(step);
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let start = Instant::now();
        for offset in 0..BATCH {
            operation((sample * BATCH + offset) as u64);
        }
        samples.push(start.elapsed().as_nanos());
    }
    start_counting();
    for step in 0..BATCH {
        operation(step as u64);
    }
    let counts = stop_counting();
    assert_eq!(
        counts,
        Counts {
            allocations: 0,
            reallocations: 0,
            deallocations: 0,
            requested_bytes: 0
        }
    );
    Measurements(samples)
}
impl Measurements {
    pub(super) fn report(mut self, prefix: &str) {
        let raw = self
            .0
            .iter()
            .map(u128::to_string)
            .collect::<Vec<_>>()
            .join(";");
        self.0.sort_unstable();
        let total: u128 = self.0.iter().sum();
        let throughput = (SAMPLES * BATCH) as u128 * 1_000_000_000 / total;
        println!(
            "{prefix},{},{},{},0,{total},{SAMPLES},{BATCH},{throughput},{raw}",
            self.0[50] / BATCH as u128,
            self.0[95] / BATCH as u128,
            self.0[99] / BATCH as u128
        );
    }
}
