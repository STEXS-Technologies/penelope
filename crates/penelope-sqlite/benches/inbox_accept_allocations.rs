//! Rust allocation traffic for duplicate input admission; no timing claims.
#[path = "support/task_read_allocator.rs"]
mod allocator;
#[path = "support/inbox_fixture.rs"]
mod fixture;
fn main() -> Result<(), fixture::Error> {
    fixture::run(|consumed, call| {
        for sample in 0..32 {
            let (result, allocations, reallocations, requested_bytes) = allocator::measure(call);
            result?;
            println!(
                "{}",
                serde_json::json!({"consumed":consumed,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes})
            );
        }
        Ok(())
    })
}
