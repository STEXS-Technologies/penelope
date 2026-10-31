//! Ordinary allocator latency for canonical effect-key bytes.
#[path = "../tests/support/effect_key_fixture.rs"]
mod fixture;
use penelope_domain::ContentDigest;
use std::{hint::black_box, time::Instant};
fn main() {
    for (profile, lengths) in fixture::PROFILES {
        for (kind, category) in fixture::KINDS.into_iter().enumerate() {
            let key = fixture::key(lengths, category, u32::MAX, 255);
            let expected = key.canonical_bytes();
            let checksum = ContentDigest::sha256(&expected);
            for _ in 0..100 {
                black_box(black_box(&key).canonical_bytes());
            }
            let iterations = 100000;
            let start = Instant::now();
            for _ in 0..iterations {
                black_box(black_box(&key).canonical_bytes());
            }
            let elapsed_ns = start.elapsed().as_nanos();
            println!(
                "{}",
                serde_json::json!({"profile":profile,"kind":kind,"iterations":iterations,"elapsed_ns":elapsed_ns,"encoded_len":expected.len(),"checksum":checksum})
            );
        }
    }
}
