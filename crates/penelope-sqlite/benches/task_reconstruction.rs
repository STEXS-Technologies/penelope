//! Ordinary-allocator latency for full authorization replay on an exact retry.
#[path = "support/task_reconstruction_fixture.rs"]
mod fixture;
fn main() {
    fixture::run(|history, removed_bytes, retry| {
        let started = std::time::Instant::now();
        for _ in 0..250 {
            std::hint::black_box(retry());
        }
        let elapsed_ns = started.elapsed().as_nanos();
        assert_eq!(
            retry(),
            penelope_core::task::runtime::StartReservation::AlreadyReserved
        );
        println!(
            "{}",
            serde_json::json!({"history":history,"iterations":250,"elapsed_ns":elapsed_ns,"expected_removed_bytes":removed_bytes})
        );
    });
}
