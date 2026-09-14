#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{ContentDigest, ManualReviewDtoV1};
use penelope_ports::{ManualReviewClaimV1, ManualReviewDecisionV1};

fuzz_target!(|data: &[u8]| {
    if let Ok(claim) = serde_json::from_slice::<ManualReviewClaimV1>(data) {
        let review = ManualReviewDtoV1::new(
            claim.scope.clone(),
            claim.review_id.clone(),
            0,
            ContentDigest([0; 32]),
        );
        let _ = claim.validate_for(&review);
    }
    if let Ok(decision) = serde_json::from_slice::<ManualReviewDecisionV1>(data) {
        let review = ManualReviewDtoV1::new(
            decision.scope.clone(),
            decision.review_id.clone(),
            0,
            decision.evidence_digest,
        );
        let _ = decision.validate_for(&review);
    }
});
