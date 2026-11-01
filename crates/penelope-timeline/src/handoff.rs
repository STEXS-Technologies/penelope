//! Finalized-effect durable admission. Uncertain acknowledgements retain the intent.
use crate::*;
use thiserror::Error;

/// A borrow made eligible only by an independently finalized live prefix.
/// The descriptor remains stable across admission retries and ACK loss.
pub struct EligibleEffect<'payload, Effect> {
    pub(crate) descriptor: EffectDescriptor,
    pub(crate) payload: &'payload Effect,
}
impl<Effect> EligibleEffect<'_, Effect> {
    /// Exact origin, scoped identity and payload binding to commit durably.
    #[must_use]
    pub const fn descriptor(&self) -> EffectDescriptor {
        self.descriptor
    }
    /// Verified consumer payload; the adapter must preserve its descriptor binding.
    #[must_use]
    pub const fn payload(&self) -> &Effect {
        self.payload
    }
}

/// Authoritatively confirmed durable admission, not proof of external execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectAdmissionReceipt {
    pub descriptor: EffectDescriptor,
    pub durable_commit: penelope_core::task::Digest,
}

/// Consumer adapter for an atomic durable inbox/outbox/process-commit boundary.
/// It must bind/deduplicate stable scope+effect identity and payload, and look up
/// authoritative committed state after ACK uncertainty. Exactly-once delivery
/// across independent stores or a non-cooperating endpoint is not implied.
pub trait EffectHandoff<Effect> {
    type Error;
    /// Admits one effect durably, or verifies an identical already committed retry.
    /// # Errors
    /// Retains the intent on unavailable/ambiguous admission; no ACK is fabricated.
    fn submit(
        &mut self,
        effect: EligibleEffect<'_, Effect>,
    ) -> Result<EffectAdmissionReceipt, Self::Error>;
}

/// Handoff failure keeps the same intent available for safe authoritative retry.
#[derive(Debug, Error)]
pub enum HandoffError<StoreError> {
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error("durable admission failed or acknowledgement is uncertain")]
    Store(StoreError),
}
