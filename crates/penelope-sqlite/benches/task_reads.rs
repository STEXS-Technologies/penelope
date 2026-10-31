//! Ordinary allocator latency; verification occurs outside each timed sample.
#[path = "support/task_read_fixture.rs"]
mod fixture;
fn main() {
    fixture::run(|name, read, expected| {
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(read());
        }
        let elapsed = start.elapsed().as_nanos();
        assert_eq!(&read(), expected);
        println!(
            "{}",
            serde_json::json!({"profile":name,"iterations":1000,"elapsed_ns":elapsed})
        );
    });
}
