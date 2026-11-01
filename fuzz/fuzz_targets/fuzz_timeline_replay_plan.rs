#![no_main]
#[allow(dead_code)]
#[path = "../../crates/penelope-timeline/tests/support/timeline_model.rs"]
mod model;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if data.len() <= 4096 {
        model::plans(data);
    }
});
