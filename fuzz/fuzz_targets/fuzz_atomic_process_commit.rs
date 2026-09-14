#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_ports::{AtomicProcessCommitV1, OutcomeReplayPageV1, OutcomeReplayRequestV1};

fuzz_target!(|data: &[u8]| {
    if let Ok(commit) = serde_json::from_slice::<AtomicProcessCommitV1>(data) {
        let _ = commit.validate();
    }
    if let Ok(request) = serde_json::from_slice::<OutcomeReplayRequestV1>(data) {
        let _ = request.validate();
    }
    if let Ok(page) = serde_json::from_slice::<OutcomeReplayPageV1>(data) {
        let _ = page.validate();
    }
});
