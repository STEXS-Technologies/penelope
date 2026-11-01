//! Fixed-capacity tentative intents. Nothing here dispatches effects.
use crate::{EffectLimits, StepContext, TimelineEffectId, TimelineError, TimelineIntentRecord};
use penelope_core::task::Digest;

/// Intent origin and stable application idempotency identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectDescriptor {
    pub origin: StepContext,
    pub identity: TimelineEffectId,
    pub digest: Digest,
    pub payload_bytes: u64,
}

impl EffectDescriptor {
    /// Canonical metadata for the independently versioned timeline record codec.
    #[must_use]
    pub const fn record(self) -> TimelineIntentRecord {
        TimelineIntentRecord {
            scope: self.origin.scope,
            branch: self.origin.branch,
            step: self.origin.step,
            revision: self.origin.revision,
            identity: self.identity,
            digest: self.digest,
            payload_bytes: self.payload_bytes,
        }
    }
}

/// A tentative, never-dispatched effect. The model supplies bounded payload storage.
#[derive(Debug)]
pub struct StagedEffect<Effect> {
    pub descriptor: EffectDescriptor,
    pub payload: Effect,
}

/// Replay-safe output sink. A rejected staging attempt poisons the entire step,
/// even if the consumer ignores the returned error.
pub trait EffectSink {
    type Effect;
    /// Stages an intent; does not call a durable store or an external endpoint.
    /// # Errors
    /// Refuses duplicate identities and configured count/byte exhaustion.
    fn stage(
        &mut self,
        identity: TimelineEffectId,
        digest: Digest,
        payload_bytes: u64,
        payload: Self::Effect,
    ) -> Result<(), TimelineError>;
}

/// Caller-sized effect storage. Rust allocation is never required by this buffer.
#[derive(Debug)]
pub struct StagedEffects<Effect, const CAPACITY: usize> {
    // Occupied slots form exactly the prefix 0..len. Append/promotion preserve
    // this invariant; handoff fills a removed slot from the final occupied slot.
    pub(crate) slots: [Option<StagedEffect<Effect>>; CAPACITY],
    pub(crate) len: usize,
    pub(crate) bytes: u64,
    pub(crate) limits: EffectLimits,
}
impl<Effect, const CAPACITY: usize> StagedEffects<Effect, CAPACITY> {
    /// Creates an empty bounded buffer; all unused slots remain uninitialized payloads.
    #[must_use]
    pub fn new(limits: EffectLimits) -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
            len: 0,
            bytes: 0,
            limits,
        }
    }
    /// Immutable tentative intents in unspecified storage order, not dispatch evidence.
    pub fn iter(&self) -> impl Iterator<Item = &StagedEffect<Effect>> {
        self.slots.iter().take(self.len).flatten()
    }
    /// Number of admitted intents.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }
    /// Whether no tentative intents have been admitted.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Admitted payload charge, not a measurement of the consumer's memory allocator.
    #[must_use]
    pub const fn payload_bytes(&self) -> u64 {
        self.bytes
    }
    pub(crate) const fn sink(
        &mut self,
        origin: StepContext,
    ) -> StepEffectSink<'_, Effect, CAPACITY> {
        StepEffectSink {
            effects: self,
            origin,
            poisoned: false,
        }
    }
    pub(crate) fn push(&mut self, effect: StagedEffect<Effect>) -> Result<(), TimelineError> {
        if self.len >= self.limits.max_intents || self.len >= CAPACITY {
            return Err(TimelineError::Capacity);
        }
        if self
            .iter()
            .any(|old| old.descriptor.identity == effect.descriptor.identity)
        {
            return Err(TimelineError::InputMismatch);
        }
        let bytes = self
            .bytes
            .checked_add(effect.descriptor.payload_bytes)
            .ok_or(TimelineError::Capacity)?;
        if bytes > self.limits.max_payload_bytes {
            return Err(TimelineError::Capacity);
        }
        let len = self.len.checked_add(1).ok_or(TimelineError::Capacity)?;
        let slot = self
            .slots
            .get_mut(self.len)
            .filter(|slot| slot.is_none())
            .ok_or(TimelineError::Capacity)?;
        *slot = Some(effect);
        self.bytes = bytes;
        self.len = len;
        Ok(())
    }
}

pub(crate) struct StepEffectSink<'effects, Effect, const CAPACITY: usize> {
    effects: &'effects mut StagedEffects<Effect, CAPACITY>,
    origin: StepContext,
    pub(crate) poisoned: bool,
}
impl<Effect, const CAPACITY: usize> EffectSink for StepEffectSink<'_, Effect, CAPACITY> {
    type Effect = Effect;
    fn stage(
        &mut self,
        identity: TimelineEffectId,
        digest: Digest,
        payload_bytes: u64,
        payload: Effect,
    ) -> Result<(), TimelineError> {
        // Poison before admission: rejecting a payload may run its destructor.
        // Even if the model catches that unwind, it cannot turn rejection into success.
        let already_poisoned = self.poisoned;
        self.poisoned = true;
        let result = self.effects.push(StagedEffect {
            descriptor: EffectDescriptor {
                origin: self.origin,
                identity,
                digest,
                payload_bytes,
            },
            payload,
        });
        if result.is_ok() {
            self.poisoned = already_poisoned;
        }
        result
    }
}
