//! Ordinary allocation: owned key control and action-to-key byte construction.
#[path = "../tests/support/action_effect_fixture.rs"]
mod action_fixture;
#[path = "../tests/support/effect_key_fixture.rs"]
mod fixture;
use penelope_domain::ContentDigest;
use std::{hint::black_box, time::Instant};
fn main() {
    for (profile, lengths) in fixture::PROFILES {
        for (kind, category) in fixture::KINDS.into_iter().enumerate() {
            let key = fixture::key(lengths, category, u32::MAX, 255);
            let action = action_fixture::action(key.clone());
            let expected = key.canonical_bytes();
            let checksum = ContentDigest::sha256(&expected);
            for path in ["key-control", "action"] {
                let encode = || {
                    if path == "key-control" {
                        black_box(&key).canonical_bytes()
                    } else {
                        black_box(&action).canonical_effect_key_bytes()
                    }
                };
                assert_eq!(encode(), expected);
                for _ in 0..100 {
                    black_box(encode());
                }
                let iterations = 100000;
                let start = Instant::now();
                for _ in 0..iterations {
                    black_box(encode());
                }
                let elapsed_ns = start.elapsed().as_nanos();
                println!(
                    "{}",
                    serde_json::json!({"profile":profile,"kind":kind,"path":path,"iterations":iterations,"elapsed_ns":elapsed_ns,"encoded_len":expected.len(),"checksum":checksum})
                );
            }
        }
    }
}
