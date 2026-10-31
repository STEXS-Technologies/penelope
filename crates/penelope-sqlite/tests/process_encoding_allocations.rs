//! Bound all-thread Rust allocation traffic for warmed fresh-commit profiles.
#[path = "../benches/support/task_read_allocator.rs"]
mod allocator;
#[path = "../benches/support/process_encoding_fixture.rs"]
mod fixture;
use penelope_ports::ProcessStore;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut calls = 0_usize;
    fixture::run(|store, batch| {
        let (result, allocations, reallocations, requested_bytes) =
            allocator::measure(|| fixture::block_on(ProcessStore::commit(store, batch)));
        let receipt = result??;
        // The fixture makes ten calls per profile, with two warmups first.
        let warmed = calls.checked_rem(10).ok_or("invalid warmup divisor")? >= 2;
        calls = calls.checked_add(1).ok_or("call counter overflow")?;
        let long = batch
            .outcomes
            .first()
            .ok_or("missing outcome")?
            .tenant_id
            .as_str()
            .len()
            > 22;
        let (max_allocations, max_reallocations, max_bytes) =
            match (long, batch.outcomes.len(), batch.actions.len()) {
                (false, 1, 0) => (18, 8, 5474),
                (false, 1, 128) => (147, 526, 193890),
                (false, 128, 0) => (18, 14, 168026),
                (false, 128, 128) => (147, 527, 358490),
                (true, 1, 0) => (41, 15, 14001),
                (true, 1, 128) => (1386, 534, 705137),
                (true, 128, 0) => (612, 21, 377720),
                (true, 128, 128) => (1957, 535, 1072952),
                _ => return Err("unexpected encoding allocation profile".into()),
            };
        if warmed
            && (allocations > max_allocations
                || reallocations > max_reallocations
                || requested_bytes > max_bytes)
        {
            return Err(format!("fresh encoding allocation budget exceeded: long={long} outcomes={} actions={} allocations={allocations} reallocations={reallocations} requested_bytes={requested_bytes}", batch.outcomes.len(), batch.actions.len()).into());
        }
        Ok((
            receipt,
            serde_json::json!({"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"guarded":warmed}),
        ))
    })
}
