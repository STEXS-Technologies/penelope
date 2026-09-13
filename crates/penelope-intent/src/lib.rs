//! Inbound transport-to-domain boundary for Penelope.
//!
//! This crate validates versioned input DTOs before the application layer sees
//! them. HTTP, RPC, broker, database, and authentication implementations do
//! not belong in this crate.

#![deny(unsafe_code)]
#![allow(clippy::must_use_candidate)]

use penelope_domain::{DomainError, ProcessInputDtoV1, ProcessInputEnvelopeV1};
use thiserror::Error;

/// Typed inbound-boundary failure.
#[derive(Debug, Error)]
pub enum IntentError {
    /// The transport bytes were not a valid JSON document for this boundary.
    #[error("invalid process input document")]
    InvalidDocument {
        /// The underlying parser failure, retained for observability only.
        #[source]
        source: serde_json::Error,
    },
    /// The document decoded but violated a typed envelope invariant.
    #[error(transparent)]
    InvalidEnvelope(#[from] DomainError),
}

/// Parses one versioned process-input document from transport bytes.
///
/// The returned domain DTO contains no transport strings. Callers must durably
/// deduplicate its `InputId` before the pure process engine consumes it.
///
/// # Errors
///
/// Returns a typed error when JSON decoding or the immutable schema
/// discriminator validation fails.
pub fn parse_process_input(bytes: &[u8]) -> Result<ProcessInputDtoV1, IntentError> {
    let envelope = serde_json::from_slice::<ProcessInputEnvelopeV1>(bytes)
        .map_err(|source| IntentError::InvalidDocument { source })?;
    envelope.validate()?;
    Ok(envelope.input)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use penelope_domain::{
        ContentDigest, InputId, ProcessId, ProcessInputKindV1, SchemaV1, TenantId,
    };

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    #[test]
    fn parse_accepts_the_explicit_process_input_schema() {
        let input = ProcessInputDtoV1::new(
            id::<TenantId>("tnt_market"),
            id::<ProcessId>("prc_trade"),
            id::<InputId>("inp_event"),
            ProcessInputKindV1::CanonicalEvent,
            ContentDigest([1; 32]),
        );
        let bytes = serde_json::to_vec(&ProcessInputEnvelopeV1::new(input.clone())).unwrap();
        assert_eq!(parse_process_input(&bytes).unwrap(), input);
    }

    #[test]
    fn parse_rejects_a_different_typed_schema() {
        let input = ProcessInputDtoV1::new(
            id::<TenantId>("tnt_market"),
            id::<ProcessId>("prc_trade"),
            id::<InputId>("inp_event"),
            ProcessInputKindV1::CanonicalEvent,
            ContentDigest([1; 32]),
        );
        let bytes = serde_json::to_vec(&ProcessInputEnvelopeV1 {
            schema: SchemaV1::ProcessOutcome,
            input,
        })
        .unwrap();
        assert!(matches!(
            parse_process_input(&bytes),
            Err(IntentError::InvalidEnvelope(
                DomainError::InvalidProcessInputSchema
            ))
        ));
    }
}
