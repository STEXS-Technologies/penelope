//! Reserve timer identity buffers without recurring allocation growth.
#[path = "../../penelope-ports/benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "support/timer_input_fixture.rs"]
mod fixture;
#[path = "../src/timer_input.rs"]
mod implementation;
fn main() -> Result<(), fixture::Error> {
    // Per-profile observed Rust allocation traffic, with 64 bytes of headroom.
    // Setup, expected output and reporting stay outside the counter.
    let byte_budgets = [
        1271, 1394, 1517, 2664, 1407, 2554, 2677, 2800, 1461, 2608, 2731, 2854, 3535, 3658, 3781,
        5952,
    ];
    let profiles = fixture::profiles()?;
    if profiles.len() != byte_budgets.len() {
        return Err("timer input allocation profile count changed".into());
    }
    for ((profile, lease), byte_budget) in profiles.into_iter().zip(byte_budgets) {
        let expected = implementation::timer_input(&lease)?;
        let (value, allocations, reallocations, requested_bytes) =
            allocator::measure(|| implementation::timer_input(&lease));
        if value? != expected {
            return Err(format!("timer input mismatch: {profile}").into());
        }
        if allocations > 6 || reallocations > 4 || requested_bytes > byte_budget {
            return Err(format!(
                "timer input allocation budget: {profile} allocations={allocations} reallocations={reallocations} requested_bytes={requested_bytes} byte_budget={byte_budget}"
            )
            .into());
        }
    }
    println!("timer_input_allocation_profiles=16 budgets_pass=true exact_output=true");
    Ok(())
}
