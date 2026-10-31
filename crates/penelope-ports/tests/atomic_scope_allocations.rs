//! Bound single-outcome validation allocations across action counts and identifier widths.
#![allow(unsafe_code, clippy::unwrap_used)]
#[path = "../benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "support/atomic_validation_fixture.rs"]
mod fixture;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    fixture::commit(0).validate()?;
    for long in [false, true] {
        for actions in [0, 1, 16, 128] {
            for varying in [false, true] {
                let mut batch = fixture::commit_with_identifiers(actions, long);
                if varying {
                    fixture::vary_payloads(&mut batch);
                }
                let (result, allocations, reallocations, requested_bytes) =
                    allocator::measure(|| batch.validate());
                result?;
                let (allocation_budget, byte_budget) = if long { (4, 512) } else { (0, 0) };
                if allocations > allocation_budget
                    || reallocations != 0
                    || requested_bytes > byte_budget
                {
                    return Err(format!("atomic scope validation exceeded its allocation budget: long={long} actions={actions} varying={varying} allocations={allocations} reallocations={reallocations} requested_bytes={requested_bytes}").into());
                }
                println!(
                    "atomic_scope long={long} actions={actions} varying={varying} allocations={allocations} reallocations={reallocations} requested_bytes={requested_bytes}"
                );
            }
        }
    }
    Ok(())
}
