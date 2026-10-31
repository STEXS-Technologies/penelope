//! Rust allocation traffic excluding SQLite C, measured separately from latency.
#[path = "support/task_read_allocator.rs"]
mod allocator;
#[path = "support/task_blob_fixture.rs"]
mod fixture;
fn main() {
    fixture::run(|fixture| {
        for sample in 0..16 {
            let (result, allocations, reallocations, requested_bytes) =
                allocator::measure(|| fixture.read());
            fixture.verify(result);
            println!(
                "{}",
                serde_json::json!({"size":fixture.size,"corrupt":fixture.corrupt,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes})
            );
        }
    });
}
