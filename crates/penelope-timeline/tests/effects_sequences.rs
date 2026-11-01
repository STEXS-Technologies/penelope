//! Interleaved publication, finality, sparse handoff and refusal reference model.
#[path = "support/effects_sequences.rs"]
mod sequences;
use proptest::prelude::*;
proptest! {
    #[test]
    fn pending_effects_match_reference_through_interleaved_operations(
        trace in prop::collection::vec(any::<u8>(), 0..128)
    ) { sequences::run(&trace); }
}

#[test]
fn ordered_bulk_handoff_retains_speculation_and_resumes_after_partial_failure() {
    let mut trace = vec![0; 16]; // Sixteen separate speculative origins.
    trace.push(2); // Independently finalize them.
    trace.extend([0; 8]); // Eight later speculative intents must survive.
    trace.extend([132, 131, 2, 131]); // Fail after two ACKs, retry, finalize, drain.
    sequences::run(&trace);
}
