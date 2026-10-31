//! Ordinary allocator latency; caller buffer checks are outside timing.
#![allow(clippy::expect_used)]
#[path = "support/task_blob_fixture.rs"]
mod fixture;
fn main() {
    fixture::run(|fixture| {
        let iterations = if fixture.size == 1048576 { 128 } else { 1000 };
        let expected = if fixture.corrupt {
            Err(penelope_core::task::runtime::TaskPortError::Corrupt)
        } else {
            Ok(usize::try_from(fixture.size).expect("size fits"))
        };
        let started = std::time::Instant::now();
        for _ in 0..iterations {
            assert_eq!(std::hint::black_box(fixture.read()), expected);
        }
        let elapsed_ns = started.elapsed().as_nanos();
        let result = fixture.read();
        fixture.verify(result);
        println!(
            "{}",
            serde_json::json!({"size":fixture.size,"corrupt":fixture.corrupt,"iterations":iterations,"elapsed_ns":elapsed_ns})
        );
    });
}
