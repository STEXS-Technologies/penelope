#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_ports::{AtomicProcessCommit, OutcomeReplayPage, OutcomeReplayRequest};

fuzz_target!(|data: &[u8]| {
    if let Ok(commit) = serde_json::from_slice::<AtomicProcessCommit>(data) {
        let _ = commit.validate();
    }
    if let Ok(request) = serde_json::from_slice::<OutcomeReplayRequest>(data) {
        let _ = request.validate();
    }
    if let Ok(page) = serde_json::from_slice::<OutcomeReplayPage>(data) {
        let _ = page.validate();
    }
});
