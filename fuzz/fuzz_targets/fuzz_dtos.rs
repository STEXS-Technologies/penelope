#![no_main]

use libfuzzer_sys::fuzz_target;
use penelope_domain::{
    CanonicalCommand, CanonicalEvent, CanonicalWireBytes, DefinitionMigration, LogicalTime,
    ManualReview, ProcessAction, ProcessDefinition, ProcessInput, ProcessInputEnvelope,
    ProcessOutcome, ProcessScope,
};
use penelope_ports::{
    AtomicProcessCommit, AtomicProcessCommitReceipt, AuthorizationRequirement,
    CanonicalReconciliation, CanonicalReconciliationWindow, CanonicalSubmitReceipt,
    DefinitionMigrationReceipt, DefinitionRegistrationReceipt, EffectDispatchRequest,
    ExternalEffectDisposition, ExternalEffectEvidence, ExternalEffectState, ManualReviewAuditEntry,
    ManualReviewClaim, ManualReviewControl, ManualReviewDecision, ManualReviewOperation,
    ManualReviewReceipt, ManualReviewResolution, OutboxAcknowledgement, OutboxClaimRequest,
    OutboxLease, OutboxLeaseToken, OutboxRecord, OutcomeLog, OutcomeReplayPage,
    OutcomeReplayRequest, ProcessAuthorizationDecision, ProcessAuthorizationOperation,
    ProcessAuthorizationRequest, QuotaKind, QuotaRequest, RecoveryDisposition, RedactedDiagnostic,
    TimerClaimRequest, TimerLease, TimerSchedule,
};

fuzz_target!(|data: &[u8]| {
    if let Ok(definition) = serde_json::from_slice::<ProcessDefinition>(data) {
        let _ = definition.validate();
        let _ = definition.canonical_wire_bytes();
        let _ = definition.canonical_bytes();
        let _ = definition.computed_digest();
        let _ = definition.require_canonical_digest();
    }
    if let Ok(migration) = serde_json::from_slice::<DefinitionMigration>(data) {
        let _ = migration.validate();
        let _ = migration.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<DefinitionRegistrationReceipt>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<DefinitionMigrationReceipt>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<AtomicProcessCommitReceipt>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(input) = serde_json::from_slice::<ProcessInput>(data) {
        let _ = input.validate();
        let _ = input.canonical_wire_bytes();
    }
    if let Ok(envelope) = serde_json::from_slice::<ProcessInputEnvelope>(data) {
        let _ = envelope.validate();
        let _ = envelope.canonical_wire_bytes();
    }
    if let Ok(outcome) = serde_json::from_slice::<ProcessOutcome>(data) {
        let _ = outcome.validate();
        let _ = outcome.canonical_wire_bytes();
    }
    if let Ok(action) = serde_json::from_slice::<ProcessAction>(data) {
        let _ = action.validate();
        let _ = action.canonical_wire_bytes();
        let _ = action.effect_key();
        let _ = action.scope().canonical_bytes();
        let _ = action.effect_key().canonical_bytes();
    }
    if let Ok(outbox) = serde_json::from_slice::<OutboxRecord>(data) {
        let _ = outbox.validate();
        let _ = outbox.canonical_wire_bytes();
    }
    if let Ok(claim) = serde_json::from_slice::<OutboxClaimRequest>(data) {
        let _ = claim.validate();
        let _ = claim.canonical_wire_bytes();
    }
    if let Ok(lease) = serde_json::from_slice::<OutboxLease>(data) {
        let _ = lease.validate();
        let _ = lease.canonical_wire_bytes();
    }
    if let Ok(log) = serde_json::from_slice::<OutcomeLog>(data) {
        let _ = OutcomeLog::from_ordered(log.scope().clone(), log.outcomes());
    }
    if let Ok(quota) = serde_json::from_slice::<QuotaRequest>(data) {
        let _ = quota.validate();
        let _ = quota.canonical_wire_bytes();
    }
    if let Ok(scope) = serde_json::from_slice::<ProcessScope>(data) {
        let _ = scope.canonical_wire_bytes();
        let _ = scope.canonical_bytes();
    }
    if let Ok(time) = serde_json::from_slice::<LogicalTime>(data) {
        let _ = time.canonical_wire_bytes();
    }
    if let Ok(command) = serde_json::from_slice::<CanonicalCommand>(data) {
        let _ = command.validate();
        let _ = command.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<CanonicalSubmitReceipt>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(window) = serde_json::from_slice::<CanonicalReconciliationWindow>(data) {
        let _ = window.canonical_wire_bytes();
    }
    if let Ok(event) = serde_json::from_slice::<CanonicalEvent>(data) {
        let _ = event.validate();
        let _ = event.canonical_wire_bytes();
    }
    if let Ok(review) = serde_json::from_slice::<ManualReview>(data) {
        let _ = review.validate();
        let _ = review.canonical_wire_bytes();
    }
    if let Ok(claim) = serde_json::from_slice::<ManualReviewClaim>(data) {
        let _ = claim.canonical_wire_bytes();
    }
    if let Ok(decision) = serde_json::from_slice::<ManualReviewDecision>(data) {
        let _ = decision.canonical_wire_bytes();
    }
    if let Ok(diagnostic) = serde_json::from_slice::<RedactedDiagnostic>(data) {
        let _ = diagnostic.canonical_wire_bytes();
    }
    if let Ok(request) = serde_json::from_slice::<TimerClaimRequest>(data) {
        let _ = request.validate();
        let _ = request.canonical_wire_bytes();
    }
    if let Ok(lease) = serde_json::from_slice::<TimerLease>(data) {
        let _ = lease.validate_at(LogicalTime(0));
        let _ = lease.canonical_wire_bytes();
    }
    if let Ok(receipt) = serde_json::from_slice::<ManualReviewReceipt>(data) {
        let _ = receipt.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ManualReviewOperation>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ManualReviewAuditEntry>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<CanonicalReconciliation>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<RecoveryDisposition>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<EffectDispatchRequest>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ExternalEffectEvidence>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ExternalEffectState>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ExternalEffectDisposition>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<TimerSchedule>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ProcessAuthorizationRequest>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ProcessAuthorizationDecision>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<AtomicProcessCommit>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<OutcomeReplayRequest>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<OutcomeReplayPage>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<OutboxAcknowledgement>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<OutboxLeaseToken>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ManualReviewControl>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ManualReviewResolution>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<ProcessAuthorizationOperation>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<AuthorizationRequirement>(data) {
        let _ = value.canonical_wire_bytes();
    }
    if let Ok(value) = serde_json::from_slice::<QuotaKind>(data) {
        let _ = value.canonical_wire_bytes();
    }
});
