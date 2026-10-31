//! Allocation traffic only, including owned identity construction on reference.
#[path = "../tests/support/action_effect_fixture.rs"]
mod action_fixture;
#[path = "../../penelope-ports/benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "../tests/support/effect_key_fixture.rs"]
mod fixture;
fn main() {
    for (profile, lengths) in fixture::PROFILES {
        for (kind, category) in fixture::KINDS.into_iter().enumerate() {
            let key = fixture::key(lengths, category, u32::MAX, 255);
            let action = action_fixture::action(key.clone());
            let expected = key.canonical_bytes();
            for path in ["key-control", "action"] {
                let (bytes, allocations, reallocations, requested_bytes) =
                    allocator::measure(|| {
                        if path == "key-control" {
                            key.canonical_bytes()
                        } else {
                            action.canonical_effect_key_bytes()
                        }
                    });
                assert_eq!(bytes, expected);
                println!(
                    "{}",
                    serde_json::json!({"profile":profile,"kind":kind,"path":path,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"encoded_len":bytes.len(),"checksum":penelope_domain::ContentDigest::sha256(&bytes)})
                );
            }
        }
    }
}
