#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_intent::parse_process_input;

fuzz_target!(|data: &[u8]| {
    let _ = parse_process_input(data);
});
