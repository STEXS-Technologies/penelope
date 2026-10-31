//! Direct action encoding must allocate only the canonical output vector.
#[path = "support/action_effect_fixture.rs"]
mod action_fixture;
#[path = "../../penelope-ports/benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "support/effect_key_fixture.rs"]
mod fixture;
fn main() {
    for (profile, lengths) in fixture::PROFILES {
        for (kind, category) in fixture::KINDS.into_iter().enumerate() {
            let key = fixture::key(lengths, category, u32::MAX, 255);
            let expected = key.canonical_bytes();
            let action = action_fixture::action(key);
            let (bytes, allocations, reallocations, requested_bytes) =
                allocator::measure(|| action.canonical_effect_key_bytes());
            assert_eq!(bytes, expected);
            assert_eq!(
                (allocations, reallocations, requested_bytes),
                (1, 0, bytes.len()),
                "{profile} kind={kind}"
            );
        }
    }
    println!("action_effect_profiles=20 allocations=1 reallocations=0 exact_output=true");
}
