//! Standalone guard: validation must not allocate for any fixture profile.
#[path = "../benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "support/timer_claim_fixture.rs"]
mod fixture;
fn main() {
    let profiles = fixture::profiles();
    assert_eq!(profiles.len(), 32);
    for (profile, lease, request) in profiles {
        let (_result, allocations, reallocations, requested_bytes) =
            allocator::measure(|| lease.validate_for_claim(&request));
        assert_eq!(
            (allocations, reallocations, requested_bytes),
            (0, 0, 0),
            "{profile}"
        );
    }
    println!("timer_claim_profiles=32 allocations=0 reallocations=0 requested_bytes=0");
}
