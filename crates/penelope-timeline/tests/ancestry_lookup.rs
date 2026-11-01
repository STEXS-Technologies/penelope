//! Ancestry range shortcuts preserve selection, child appends and materialized roots.
#![allow(clippy::indexing_slicing)]
#[path = "support/ancestry_lookup.rs"]
mod ancestry;
use proptest::prelude::*;
proptest! {
    #[test]
    fn nested_canonical_inputs_match_independent_branch_versions(data in prop::collection::vec(any::<u8>(), 0..96)) {
        ancestry::run(&data);
    }
}
#[test]
fn deep_state_input_and_mixed_corrections_preserve_child_appends() {
    for mode in [0, 1, 85, 170, 255] {
        let mut data = [mode; 96];
        data[1] = 4;
        ancestry::run(&data);
    }
}
