#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    CanonicalCommandDtoV1, CanonicalEventDtoV1, LogicalTimeV1, ManualReviewDtoV1,
    ProcessActionDtoV1, ProcessDefinitionDtoV1, ProcessInputDtoV1, ProcessInputEnvelopeV1,
    ProcessOutcomeDtoV1, ProcessScopeV1,
};
use penelope_ports::{OutboxClaimRequestV1, OutboxLeaseV1, OutboxRecordV1};

fuzz_target!(|data: &[u8]| {
    if let Ok(definition) = serde_json::from_slice::<ProcessDefinitionDtoV1>(data) {
        let _ = definition.validate();
    }
    if let Ok(input) = serde_json::from_slice::<ProcessInputDtoV1>(data) {
        let _ = input.validate();
    }
    if let Ok(envelope) = serde_json::from_slice::<ProcessInputEnvelopeV1>(data) {
        let _ = envelope.validate();
    }
    if let Ok(outcome) = serde_json::from_slice::<ProcessOutcomeDtoV1>(data) {
        let _ = outcome.validate();
    }
    if let Ok(action) = serde_json::from_slice::<ProcessActionDtoV1>(data) {
        let _ = action.validate();
        let _ = action.effect_key();
    }
    if let Ok(outbox) = serde_json::from_slice::<OutboxRecordV1>(data) {
        let _ = outbox.validate();
    }
    if let Ok(claim) = serde_json::from_slice::<OutboxClaimRequestV1>(data) {
        let _ = claim.validate();
    }
    if let Ok(lease) = serde_json::from_slice::<OutboxLeaseV1>(data) {
        let _ = lease.validate();
    }
    let _ = serde_json::from_slice::<ProcessScopeV1>(data);
    let _ = serde_json::from_slice::<LogicalTimeV1>(data);
    if let Ok(command) = serde_json::from_slice::<CanonicalCommandDtoV1>(data) {
        let _ = command.validate();
    }
    if let Ok(event) = serde_json::from_slice::<CanonicalEventDtoV1>(data) {
        let _ = event.validate();
    }
    if let Ok(review) = serde_json::from_slice::<ManualReviewDtoV1>(data) {
        let _ = review.validate();
    }
});
