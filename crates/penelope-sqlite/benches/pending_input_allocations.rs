//! Rust allocation traffic for pending-input discovery; no timing claims.
#[path = "support/task_read_allocator.rs"]
mod allocator;
#[path = "support/pending_fixture.rs"]
mod fixture;
fn main() -> Result<(), fixture::Error> {
    fixture::run(|pending, call| {
        for sample in 0..32 {
            let (result, allocations, reallocations, requested_bytes) = allocator::measure(call);
            result?;
            println!(
                "{}",
                serde_json::json!({"pending":pending,"sample":sample,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes})
            );
        }
        Ok(())
    })
}
