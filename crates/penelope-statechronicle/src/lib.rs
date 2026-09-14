//! StateChronicle integration adapter for Penelope.
//!
//! This crate owns only the adapter boundary. It will submit a persisted
//! Penelope action through StateChronicle's verified durable command path and
//! translate a verified, committed canonical event into a Penelope inbox input.
//! It must not perform direct database writes, invent a successful command from
//! an HTTP response, or permit replay to issue a second canonical command.
//!
//! No client implementation exists here. The contract and implementation order
//! are in the repository README and TODO.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use penelope_domain::{
    ActionId, CanonicalEventDtoV1, ContentDigest, DomainError, OperationId, ProcessScopeV1,
    ResourceId, validate_canonical_resource_scope,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Immutable correlation requirements for a submitted canonical command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalCommandExpectationV1 {
    /// Immutable Penelope process and definition scope that authorized this command.
    pub scope: ProcessScopeV1,
    /// Penelope action ID reused as canonical idempotency identity.
    pub action_id: ActionId,
    /// Registered canonical operation expected to commit.
    pub operation: OperationId,
    /// Exact canonical resource scope expected to commit.
    pub resource_ids: Vec<ResourceId>,
    /// Exact redacted digest expected from the committed canonical result.
    pub expected_event_payload_digest: ContentDigest,
}

impl CanonicalCommandExpectationV1 {
    /// Validates the expected canonical resource scope before correlation.
    ///
    /// # Errors
    ///
    /// Returns a typed domain error for oversized or duplicate resource IDs.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_canonical_resource_scope(&self.resource_ids)
    }
}

/// A canonical event that passed Penelope's correlation checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedCanonicalEventV1 {
    /// Pinned Penelope scope that authorized and correlated this event.
    pub scope: ProcessScopeV1,
    /// The event confirmed against the command expectation.
    pub event: CanonicalEventDtoV1,
}

/// Typed canonical-event correlation failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CorrelationError {
    /// Expected command scope violated the canonical resource-scope invariant.
    #[error("canonical command expectation is invalid")]
    InvalidExpectation {
        /// Underlying typed domain validation error.
        #[source]
        source: DomainError,
    },
    /// Received committed event violated the canonical resource-scope invariant.
    #[error("canonical event is invalid")]
    InvalidEvent {
        /// Underlying typed domain validation error.
        #[source]
        source: DomainError,
    },
    /// Event tenant does not match the authorized process scope.
    #[error("canonical event tenant does not match command expectation")]
    TenantMismatch,
    /// Event action identity does not match the submitted action.
    #[error("canonical event action does not match command expectation")]
    ActionMismatch,
    /// Event operation does not match the submitted canonical operation.
    #[error("canonical event operation does not match command expectation")]
    OperationMismatch,
    /// Event resource scope does not exactly match the submitted scope.
    #[error("canonical event resource scope does not match command expectation")]
    ResourceScopeMismatch,
    /// Committed canonical result digest differs from the exact expected result.
    #[error("canonical event payload digest does not match command expectation")]
    PayloadDigestMismatch,
}

/// Correlates an event from a trusted committed-event stream to a previously
/// submitted canonical command.
///
/// This validates correlation only. An adapter must obtain `event` from a
/// verified committed StateChronicle outbox source; a request response or an
/// untrusted transport payload is not evidence of a committed canonical fact.
///
/// # Errors
///
/// Returns a typed mismatch when any tenant, action, operation, or exact
/// resource scope does not match the submitted command expectation.
pub fn verify_committed_event(
    expected: &CanonicalCommandExpectationV1,
    event: CanonicalEventDtoV1,
) -> Result<VerifiedCanonicalEventV1, CorrelationError> {
    expected
        .validate()
        .map_err(|source| CorrelationError::InvalidExpectation { source })?;
    event
        .validate()
        .map_err(|source| CorrelationError::InvalidEvent { source })?;
    if event.tenant_id != expected.scope.tenant_id {
        return Err(CorrelationError::TenantMismatch);
    }
    if event.action_id != expected.action_id {
        return Err(CorrelationError::ActionMismatch);
    }
    if event.operation != expected.operation {
        return Err(CorrelationError::OperationMismatch);
    }
    if event.resource_ids != expected.resource_ids {
        return Err(CorrelationError::ResourceScopeMismatch);
    }
    if event.payload_digest != expected.expected_event_payload_digest {
        return Err(CorrelationError::PayloadDigestMismatch);
    }
    Ok(VerifiedCanonicalEventV1 {
        scope: expected.scope.clone(),
        event,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use penelope_domain::ContentDigest;

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    fn expectation() -> CanonicalCommandExpectationV1 {
        CanonicalCommandExpectationV1 {
            scope: ProcessScopeV1::new(
                id("tnt_market"),
                id("prc_trade"),
                id("def_trade"),
                id("dfv_one"),
                ContentDigest([9; 32]),
            ),
            action_id: id("act_settle"),
            operation: id("op_settle"),
            resource_ids: vec![id("res_asset_a"), id("res_asset_b")],
            expected_event_payload_digest: ContentDigest([1; 32]),
        }
    }

    fn event() -> CanonicalEventDtoV1 {
        CanonicalEventDtoV1::new(
            id("tnt_market"),
            id("cev_outbox"),
            id("act_settle"),
            id("cmt_commit"),
            7,
            id("op_settle"),
            vec![id("res_asset_a"), id("res_asset_b")],
            ContentDigest([1; 32]),
        )
        .unwrap()
    }

    #[test]
    fn verified_event_requires_full_correlation() {
        let verified = verify_committed_event(&expectation(), event()).unwrap();
        assert_eq!(verified.event.commit_sequence, 7);
        assert_eq!(verified.scope.process_id, id("prc_trade"));
    }

    #[test]
    fn wrong_action_cannot_advance_a_saga() {
        let mut received = event();
        received.action_id = id("act_other");
        let error = verify_committed_event(&expectation(), received).unwrap_err();
        assert_eq!(error, CorrelationError::ActionMismatch);
    }

    #[test]
    fn duplicate_resources_in_received_evidence_are_rejected_before_correlation() {
        let mut received = event();
        received.resource_ids.push(id("res_asset_a"));
        let error = verify_committed_event(&expectation(), received).unwrap_err();
        assert!(matches!(
            error,
            CorrelationError::InvalidEvent {
                source: DomainError::DuplicateCanonicalResourceId
            }
        ));
    }

    #[test]
    fn duplicate_resources_in_expected_scope_are_rejected_before_correlation() {
        let mut expected = expectation();
        expected.resource_ids.push(id("res_asset_a"));
        let error = verify_committed_event(&expected, event()).unwrap_err();
        assert!(matches!(
            error,
            CorrelationError::InvalidExpectation {
                source: DomainError::DuplicateCanonicalResourceId
            }
        ));
    }

    #[test]
    fn wrong_committed_result_digest_cannot_advance_a_saga() {
        let mut received = event();
        received.payload_digest = ContentDigest([2; 32]);
        assert_eq!(
            verify_committed_event(&expectation(), received).unwrap_err(),
            CorrelationError::PayloadDigestMismatch
        );
    }
}
