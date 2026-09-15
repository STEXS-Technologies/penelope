#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{ContentDigest, LogicalTime, ManualReview};
use penelope_ports::{ManualReviewClaim, ManualReviewDecision};

fuzz_target!(|data: &[u8]| {
    if let Ok(claim) = serde_json::from_slice::<ManualReviewClaim>(data) {
        let review = ManualReview::new(
            claim.scope.clone(),
            claim.review_id.clone(),
            0,
            Some(LogicalTime(0)),
            ContentDigest([0; 32]),
        );
        let _ = claim.validate_for(&review);
    }
    if let Ok(decision) = serde_json::from_slice::<ManualReviewDecision>(data) {
        let review = ManualReview::new(
            decision.scope.clone(),
            decision.review_id.clone(),
            0,
            Some(LogicalTime(0)),
            decision.evidence_digest,
        );
        let _ = decision.validate_for(&review);
    }
});
