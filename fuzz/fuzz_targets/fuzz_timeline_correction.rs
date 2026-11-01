#![no_main]
#[path = "../../crates/penelope-timeline/tests/support/ancestry_lookup.rs"]
mod ancestry;
#[path = "../../crates/penelope-timeline/tests/support/history_lookup.rs"]
mod lookup;
#[allow(dead_code)]
#[path = "../../crates/penelope-timeline/tests/support/timeline_model.rs"]
mod model;
#[path = "../../crates/penelope-timeline/tests/support/snapshot_lookup.rs"]
mod snapshots;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if data.len() <= 4096 {
        model::correction(data);
        lookup::run(data);
        ancestry::run(data);
        snapshots::run(data);
    }
});
