//! Rust allocation traffic measured separately from latency, excluding SQLite C.
#[path = "support/task_read_allocator.rs"]
mod allocator;
#[path = "support/task_read_fixture.rs"]
mod fixture;
fn main() {
    fixture::run(|name, read, expected| {
        for sample in 0..16 {
            let (result, allocations, reallocations, requested_bytes) =
                allocator::measure(&mut *read);
            assert_eq!(&result, expected);
            println!(
                "{}",
                serde_json::json!({"profile":name,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes})
            );
        }
    });
}
