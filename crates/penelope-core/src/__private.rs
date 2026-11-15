//! Internal cross-crate primitives; not part of the supported application API.

/// Preflights append-only intent metadata without taking ownership of payloads.
/// Count precedes identity, which precedes byte accounting. Nothing is mutated;
/// the supplied errors preserve each domain's capacity/duplicate categories;
/// adapters must pre-poison before a rejected payload can run its destructor.
/// # Errors
/// Refuses full storage, duplicate identity, overflow or charged-byte exhaustion.
#[inline]
pub fn admit_intent<const CAPACITY: usize, Identity: Copy + Eq, Error: Copy>(
    len: usize,
    bytes: u64,
    limits: (usize, u64),
    mut identities: impl Iterator<Item = Identity>,
    identity: Identity,
    charge: u64,
    errors: (Error, Error),
) -> Result<(usize, u64), Error> {
    let (capacity, duplicate) = errors;
    if len >= CAPACITY || len >= limits.0 {
        return Err(capacity);
    }
    if identities.any(|old| old == identity) {
        return Err(duplicate);
    }
    let bytes = bytes.checked_add(charge).ok_or(capacity)?;
    if bytes > limits.1 {
        return Err(capacity);
    }
    let len = len.checked_add(1).ok_or(capacity)?;
    Ok((len, bytes))
}
