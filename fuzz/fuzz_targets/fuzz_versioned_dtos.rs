#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    CanonicalCommandDtoV1, CanonicalEventDtoV1, CanonicalWireBytesV1, DefinitionMigrationV1,
    LogicalTimeV1, ManualReviewDtoV1, ProcessActionDtoV1, ProcessDefinitionDtoV1,
    ProcessInputDtoV1, ProcessInputEnvelopeV1, ProcessOutcomeDtoV1, ProcessScopeV1,
};
use penelope_ports::{
    CanonicalSubmitReceiptV1, DefinitionRegistrationReceiptV1, OutboxClaimRequestV1, OutboxLeaseV1,
    OutboxRecordV1, OutcomeLogV1, QuotaRequestV1,
};

fuzz_target!(|data: &[u8]| {
    if let Ok(definition) = serde_json::from_slice::<ProcessDefinitionDtoV1>(data) {
        let _ = definition.validate();
        let _ = definition.canonical_wire_bytes();
        let _ = definition.canonical_bytes();
        let _ = definition.computed_digest();
        let _ = definition.require_canonical_digest();
    }
    if let Ok(migration) = serde_json::from_slice::<DefinitionMigrationV1>(data) {
        let _ = migration.validate();
        let _ = migration.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<DefinitionRegistrationReceiptV1>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(input) = serde_json::from_slice::<ProcessInputDtoV1>(data) {
        let _ = input.validate();
        let _ = input.canonical_wire_bytes();
    }
    if let Ok(envelope) = serde_json::from_slice::<ProcessInputEnvelopeV1>(data) {
        let _ = envelope.validate();
        let _ = envelope.canonical_wire_bytes();
    }
    if let Ok(outcome) = serde_json::from_slice::<ProcessOutcomeDtoV1>(data) {
        let _ = outcome.validate();
        let _ = outcome.canonical_wire_bytes();
    }
    if let Ok(action) = serde_json::from_slice::<ProcessActionDtoV1>(data) {
        let _ = action.validate();
        let _ = action.canonical_wire_bytes();
        let _ = action.effect_key();
        let _ = action.scope().canonical_bytes();
        let _ = action.effect_key().canonical_bytes();
    }
    if let Ok(outbox) = serde_json::from_slice::<OutboxRecordV1>(data) {
        let _ = outbox.validate();
        let _ = outbox.canonical_wire_bytes();
    }
    if let Ok(claim) = serde_json::from_slice::<OutboxClaimRequestV1>(data) {
        let _ = claim.validate();
        let _ = claim.canonical_wire_bytes();
    }
    if let Ok(lease) = serde_json::from_slice::<OutboxLeaseV1>(data) {
        let _ = lease.validate();
        let _ = lease.canonical_wire_bytes();
    }
    if let Ok(log) = serde_json::from_slice::<OutcomeLogV1>(data) {
        let _ = OutcomeLogV1::from_ordered(log.scope().clone(), log.outcomes());
    }
    if let Ok(quota) = serde_json::from_slice::<QuotaRequestV1>(data) {
        let _ = quota.validate();
        let _ = quota.canonical_wire_bytes();
    }
    if let Ok(scope) = serde_json::from_slice::<ProcessScopeV1>(data) {
        let _ = scope.canonical_wire_bytes();
        let _ = scope.canonical_bytes();
    }
    if let Ok(time) = serde_json::from_slice::<LogicalTimeV1>(data) {
        let _ = time.canonical_wire_bytes();
    }
    if let Ok(command) = serde_json::from_slice::<CanonicalCommandDtoV1>(data) {
        let _ = command.validate();
        let _ = command.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<CanonicalSubmitReceiptV1>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(event) = serde_json::from_slice::<CanonicalEventDtoV1>(data) {
        let _ = event.validate();
        let _ = event.canonical_wire_bytes();
    }
    if let Ok(review) = serde_json::from_slice::<ManualReviewDtoV1>(data) {
        let _ = review.validate();
        let _ = review.canonical_wire_bytes();
    }
});
