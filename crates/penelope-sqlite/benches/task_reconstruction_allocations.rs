//! Rust allocation traffic, separate from latency and excluding SQLite C allocation.
#[path = "support/task_read_allocator.rs"]
mod allocator;
#[path = "support/task_reconstruction_fixture.rs"]
mod fixture;
fn main() {
    fixture::run(|history, alternating, removed_bytes, retry| {
        for sample in 0..16 {
            let (result, allocations, reallocations, requested_bytes) =
                allocator::measure(&mut *retry);
            assert_eq!(
                result,
                penelope_core::task::runtime::StartReservation::AlreadyReserved
            );
            println!(
                "{}",
                serde_json::json!({"history":history,"alternating":alternating,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"expected_removed_bytes":removed_bytes})
            );
        }
    });
}
