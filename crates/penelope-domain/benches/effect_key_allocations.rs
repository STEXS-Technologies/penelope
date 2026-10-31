//! Allocation traffic only; kept separate from ordinary latency measurement.
#[path = "../../penelope-ports/benches/support/outcome_log_allocator.rs"]
mod allocator;
#[path = "../tests/support/effect_key_fixture.rs"]
mod fixture;
use penelope_domain::ContentDigest;
use std::hint::black_box;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for (profile, lengths) in fixture::PROFILES {
        for (kind, category) in fixture::KINDS.into_iter().enumerate() {
            let key = fixture::key(lengths, category, u32::MAX, 255);
            let expected = key.canonical_bytes();
            let checksum = ContentDigest::sha256(&expected);
            let (bytes, allocations, reallocations, requested_bytes) =
                allocator::measure(|| black_box(&key).canonical_bytes());
            if bytes != expected {
                return Err("effect-key bytes differ".into());
            }
            println!(
                "{}",
                serde_json::json!({"profile":profile,"kind":kind,"allocations":allocations,"reallocations":reallocations,"requested_bytes":requested_bytes,"encoded_len":expected.len(),"checksum":checksum})
            );
        }
    }
    Ok(())
}
