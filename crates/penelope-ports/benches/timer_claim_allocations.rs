//! Allocation traffic, separate from ordinary latency measurement.
#[path = "support/outcome_log_allocator.rs"]
mod allocator;
#[path = "../tests/support/timer_claim_fixture.rs"]
mod fixture;
fn main() {
    for (profile, lease, request) in fixture::profiles() {
        let (result, allocations, reallocations, requested_bytes) =
            allocator::measure(|| lease.validate_for_claim(&request));
        println!(
            "{}",
            serde_json::json!({"profile":profile,"result":format!("{result:?}"),"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes})
        );
    }
}
