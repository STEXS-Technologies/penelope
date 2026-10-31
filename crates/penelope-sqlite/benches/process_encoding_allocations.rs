//! All-thread Rust allocation traffic during fresh commits; no timing claims.
#[path = "support/task_read_allocator.rs"]
mod allocator;
#[path = "support/process_encoding_fixture.rs"]
mod fixture;
use penelope_ports::ProcessStore;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    fixture::run(|store, batch| {
        let (result, allocations, reallocations, requested_bytes) =
            allocator::measure(|| fixture::block_on(ProcessStore::commit(store, batch)));
        Ok((
            result??,
            serde_json::json!({"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes}),
        ))
    })
}
