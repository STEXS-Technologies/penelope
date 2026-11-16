//! Generated models exercise the same independent oracle as the public fuzzer.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
#[path = "support/generated_machine.rs"]
mod model;

#[test]
fn generated_tables_and_hierarchies_match_reference_and_single_level_flat_dispatch() {
    for seed in 1..=512_u64 {
        let mut rng = seed;
        let bytes: Vec<_> = (0..138)
            .map(|_| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng.to_le_bytes()[0]
            })
            .collect();
        model::check(&bytes);
    }
    for length in 0..=42 {
        model::check(&vec![0; length]);
        model::check(&vec![255; length]);
    }
}
