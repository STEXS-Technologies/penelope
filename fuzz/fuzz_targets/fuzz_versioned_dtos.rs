#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    CanonicalCommandDtoV1, CanonicalEventDtoV1, CanonicalWireBytesV1, DefinitionMigrationV1,
    LogicalTimeV1, ManualReviewDtoV1, ProcessActionDtoV1, ProcessDefinitionDtoV1,
    ProcessInputDtoV1, ProcessInputEnvelopeV1, ProcessOutcomeDtoV1, ProcessScopeV1,
};
use penelope_ports::{
    AtomicProcessCommitReceiptV1, AtomicProcessCommitV1, AuthorizationRequirementV1,
    CanonicalReconciliationV1, CanonicalReconciliationWindowV1, CanonicalSubmitReceiptV1,
    DefinitionMigrationReceiptV1, DefinitionRegistrationReceiptV1, EffectDispatchRequestV1,
    ExternalEffectDispositionV1, ExternalEffectEvidenceV1, ExternalEffectStateV1,
    ManualReviewAuditEntryV1, ManualReviewClaimV1, ManualReviewControlV1, ManualReviewDecisionV1,
    ManualReviewOperationV1, ManualReviewReceiptV1, ManualReviewResolutionV1,
    OutboxAcknowledgementV1, OutboxClaimRequestV1, OutboxLeaseTokenV1, OutboxLeaseV1,
    OutboxRecordV1, OutcomeLogV1, OutcomeReplayPageV1, OutcomeReplayRequestV1,
    ProcessAuthorizationDecisionV1, ProcessAuthorizationOperationV1, ProcessAuthorizationRequestV1,
    QuotaKindV1, QuotaRequestV1, RecoveryDispositionV1, RedactedDiagnosticV1, TimerClaimRequestV1,
    TimerLeaseV1, TimerScheduleV1,
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
    if let Ok(receipt) = serde_json::from_slice::<DefinitionMigrationReceiptV1>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<AtomicProcessCommitReceiptV1>(data) {
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
    if let Ok(window) = serde_json::from_slice::<CanonicalReconciliationWindowV1>(data) {
        let _ = window.canonical_wire_bytes();
    }
    if let Ok(event) = serde_json::from_slice::<CanonicalEventDtoV1>(data) {
        let _ = event.validate();
        let _ = event.canonical_wire_bytes();
    }
    if let Ok(review) = serde_json::from_slice::<ManualReviewDtoV1>(data) {
        let _ = review.validate();
        let _ = review.canonical_wire_bytes();
    }
    if let Ok(claim) = serde_json::from_slice::<ManualReviewClaimV1>(data) {
        let _ = claim.canonical_wire_bytes();
    }
    if let Ok(decision) = serde_json::from_slice::<ManualReviewDecisionV1>(data) {
        let _ = decision.canonical_wire_bytes();
    }
    if let Ok(diagnostic) = serde_json::from_slice::<RedactedDiagnosticV1>(data) {
        let _ = diagnostic.canonical_wire_bytes();
    }
    if let Ok(request) = serde_json::from_slice::<TimerClaimRequestV1>(data) {
        let _ = request.validate();
        let _ = request.canonical_wire_bytes();
    }
    if let Ok(lease) = serde_json::from_slice::<TimerLeaseV1>(data) {
        let _ = lease.validate_at(LogicalTimeV1(0));
        let _ = lease.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<ManualReviewReceiptV1>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ManualReviewOperationV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ManualReviewAuditEntryV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<CanonicalReconciliationV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<RecoveryDispositionV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<EffectDispatchRequestV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ExternalEffectEvidenceV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ExternalEffectStateV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ExternalEffectDispositionV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<TimerScheduleV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ProcessAuthorizationRequestV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ProcessAuthorizationDecisionV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<AtomicProcessCommitV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<OutcomeReplayRequestV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<OutcomeReplayPageV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<OutboxAcknowledgementV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<OutboxLeaseTokenV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ManualReviewControlV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ManualReviewResolutionV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ProcessAuthorizationOperationV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<AuthorizationRequirementV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<QuotaKindV1>(data) {
        let _ = value.canonical_wire_bytes();
    }
});
