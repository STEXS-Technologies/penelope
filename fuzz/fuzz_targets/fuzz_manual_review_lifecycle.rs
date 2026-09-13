#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_ports::{ManualReviewClaimV1, ManualReviewDecisionV1};

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<ManualReviewClaimV1>(data);
    let _ = serde_json::from_slice::<ManualReviewDecisionV1>(data);
});
