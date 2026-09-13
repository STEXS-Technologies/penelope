#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_ports::AtomicProcessCommitV1;

fuzz_target!(|data: &[u8]| {
    if let Ok(commit) = serde_json::from_slice::<AtomicProcessCommitV1>(data) {
        let _ = commit.validate();
    }
});
