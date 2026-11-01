//! Allocation accounting in a separate executable from latency measurements.
#[path = "support/allocator.rs"]
mod allocator;
#[path = "support/workload.rs"]
mod workload;
struct Allocations;
impl workload::Reporter for Allocations {
    fn sample<T, R>(
        &mut self,
        name: &str,
        words: usize,
        steps: usize,
        mut setup: impl FnMut() -> T,
        mut operation: impl FnMut(&mut T) -> R,
    ) {
        let mut input = setup();
        let (result, allocations, reallocations, bytes) = allocator::measure(|| {
            std::hint::black_box(operation(std::hint::black_box(&mut input)))
        });
        std::hint::black_box(result);
        assert_eq!((allocations, reallocations, bytes), (0, 0, 0));
        println!("allocation,{name},{words},{steps},{allocations},{reallocations},{bytes}");
    }
}
fn main() {
    workload::run(Allocations);
}
