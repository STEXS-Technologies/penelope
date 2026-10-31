//! Pinned timer-input builder from parent 00b79aa, retained as a compatibility oracle.
use penelope_domain::{CanonicalWireBytes, ContentDigest, ProcessInput, ProcessInputKind};
use penelope_ports::{PortError, TimerLease};
use std::fmt::Write as _;

/// Stable timer inbox input and durable payload. Delivery attempt, lease token
/// and current time are deliberately excluded so redelivery is immutable.
pub(super) fn timer_input(lease: &TimerLease) -> Result<(ProcessInput, Vec<u8>), PortError> {
    let bytes = lease
        .timer
        .canonical_wire_bytes()
        .map_err(|_invalid| PortError::Invariant)?;
    let mut key = b"penelope.timer-input.v1\0".to_vec();
    key.extend_from_slice(&lease.timer.action.canonical_effect_key_bytes());
    key.extend_from_slice(lease.timer.action.action_id.as_str().as_bytes());
    let digest = ContentDigest::sha256(&key);
    let mut identity = String::from("inp_timer_");
    for byte in digest.0 {
        write!(identity, "{byte:02x}").map_err(|_format| PortError::Invariant)?;
    }
    let action = &lease.timer.action;
    let input = ProcessInput::new(
        action.tenant_id.clone(),
        action.process_id.clone(),
        identity
            .try_into()
            .map_err(|_invalid| PortError::Invariant)?,
        ProcessInputKind::TimerFired,
        ContentDigest::sha256(&bytes),
    );
    Ok((input, bytes))
}
