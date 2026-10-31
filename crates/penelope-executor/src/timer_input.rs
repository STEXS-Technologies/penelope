//! Stable timer inbox input identity and canonical payload construction.
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
    let digest = {
        let effect_key = lease.timer.action.canonical_effect_key_bytes();
        let action_id = lease.timer.action.action_id.as_str().as_bytes();
        let prefix = b"penelope.timer-input.v1\0";
        let capacity = prefix
            .len()
            .saturating_add(effect_key.len())
            .saturating_add(action_id.len());
        let mut key = Vec::with_capacity(capacity);
        key.extend_from_slice(prefix);
        key.extend_from_slice(&effect_key);
        key.extend_from_slice(action_id);
        ContentDigest::sha256(&key)
    };
    // Ten prefix bytes plus two hexadecimal digits for each digest byte.
    let mut identity = String::with_capacity(74);
    identity.push_str("inp_timer_");
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
