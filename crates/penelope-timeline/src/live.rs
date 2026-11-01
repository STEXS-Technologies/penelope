//! Owned atomic publication profile: readers borrow head and state together.
use crate::*;

/// In-memory exclusive-ownership publication profile. There is no mutable state
/// accessor. All readers borrow the same owner; hosted sharing must use one lock
/// around this entire value, not independent locks/atomics for head and state.
/// The consumer must not retain interior-mutable aliases to published state.
pub struct LiveTimeline<State, Effect, const SESSIONS: usize, const EFFECTS: usize> {
    projection: TimelineProjection<SESSIONS>,
    state: State,
    effects: StagedEffects<Effect, EFFECTS>,
}
impl<State, Effect, const SESSIONS: usize, const EFFECTS: usize>
    LiveTimeline<State, Effect, SESSIONS, EFFECTS>
{
    /// Creates the genesis live publication; storage authentication is a consumer duty.
    #[must_use]
    pub fn new(head: TimelineHead, state: State, limits: EffectLimits) -> Self {
        Self {
            projection: TimelineProjection::new(head),
            state,
            effects: StagedEffects::new(limits),
        }
    }
    /// Reads one consistent state/head pair under a single ownership boundary.
    #[must_use]
    pub const fn read(&self) -> (TimelineHead, &State) {
        (self.projection.head(), &self.state)
    }
    pub(crate) const fn effect_limits(&self) -> EffectLimits {
        self.effects.limits
    }
    /// Tentative intents are observable for diagnostics, not dispatch eligibility.
    pub fn tentative_effects(&self) -> impl Iterator<Item = &StagedEffect<Effect>> {
        self.effects.iter()
    }
    /// Reserves an immutable candidate branch while preserving live state/head.
    /// # Errors
    /// Refuses stale/invalid plans, authority ordering, counter or capacity exhaustion.
    pub fn reserve(
        &mut self,
        plan: &ReplayPlan<'_>,
        observation: AuthorityObservation,
    ) -> Result<BranchReservation, TimelineError> {
        self.projection.reserve(plan, observation)
    }
    /// Releases a stale/aborted reservation without ever reusing its branch identity.
    /// # Errors
    /// Refuses an absent or already consumed reservation.
    pub fn abort(&mut self, reservation: BranchReservation) -> Result<(), TimelineError> {
        self.projection.abort(reservation)
    }
    /// Atomically publishes validated scratch state and its candidate metadata.
    /// No fallible operation or consumer destructor runs between head/state swaps.
    /// Returned superseded payloads are dropped only after publication is coherent.
    /// # Errors
    /// Refuses stale/reused reservations, duplicate surviving intent IDs or merged quotas.
    pub fn promote(
        &mut self,
        mut candidate: ReadyCandidate<State, Effect, EFFECTS>,
    ) -> Result<TimelineHead, TimelineError> {
        self.projection.promotion_head(candidate.reservation)?;
        let mut count = candidate.effects.len;
        let mut bytes = candidate.effects.bytes;
        for effect in self
            .effects
            .iter()
            .filter(|effect| effect.descriptor.origin.step < candidate.replace_from)
        {
            count = count.checked_add(1).ok_or(TimelineError::Capacity)?;
            bytes = bytes
                .checked_add(effect.descriptor.payload_bytes)
                .ok_or(TimelineError::Capacity)?;
            if candidate
                .effects
                .iter()
                .any(|new| new.descriptor.identity == effect.descriptor.identity)
            {
                return Err(TimelineError::InputMismatch);
            }
        }
        if count > EFFECTS
            || count > self.effects.limits.max_intents
            || bytes > self.effects.limits.max_payload_bytes
        {
            return Err(TimelineError::Capacity);
        }
        let head = self.projection.promote(candidate.reservation)?;
        // All remaining operations before the complete swap only move values;
        // no user callbacks, allocation, indexing, arithmetic, or Drop can fail.
        let mut vacant = candidate
            .effects
            .slots
            .iter_mut()
            .filter(|slot| slot.is_none());
        for old in self.effects.slots.iter_mut().filter(|slot| {
            slot.as_ref()
                .is_some_and(|effect| effect.descriptor.origin.step < candidate.replace_from)
        }) {
            if let Some(slot) = vacant.next() {
                *slot = old.take();
            }
        }
        candidate.effects.len = count;
        candidate.effects.bytes = bytes;
        candidate.effects.limits = self.effects.limits;
        let old_state = core::mem::replace(&mut self.state, candidate.state);
        let old_effects = core::mem::replace(&mut self.effects, candidate.effects);
        // Coherent publication precedes arbitrary consumer destructors.
        drop(old_state);
        drop(old_effects);
        Ok(head)
    }
    /// Admits an authenticated authority observation independently of effect finality.
    /// # Errors
    /// Refuses stale head/authority, invalid scope or future step.
    pub fn observe(
        &mut self,
        expected: TimelineHead,
        observation: AuthorityObservation,
    ) -> Result<TimelineHead, TimelineError> {
        self.projection.observe(expected, observation)
    }
    /// Finalizes a prefix only after the application independently verifies finality proof.
    /// # Errors
    /// Refuses stale heads, backward/future frontier and revision exhaustion.
    pub fn finalize_effects(
        &mut self,
        expected: TimelineHead,
        through: TimelineStep,
    ) -> Result<TimelineHead, TimelineError> {
        self.projection.finalize_effects(expected, through)
    }
}

impl<State, Effect, const SESSIONS: usize, const EFFECTS: usize>
    LiveTimeline<State, Effect, SESSIONS, EFFECTS>
{
    /// Publishes a validated live increment, atomically with its tentative intents.
    /// A pinned correction is thereby superseded; it cannot overwrite this input.
    /// # Errors
    /// Refuses a stale increment, duplicate pending intent identity or merged quota.
    pub fn append(
        &mut self,
        mut step: ReadyStep<State, Effect, EFFECTS>,
    ) -> Result<TimelineHead, TimelineError> {
        let count = self
            .effects
            .len
            .checked_add(step.effects.len)
            .ok_or(TimelineError::Capacity)?;
        let bytes = self
            .effects
            .bytes
            .checked_add(step.effects.bytes)
            .ok_or(TimelineError::Capacity)?;
        if count > EFFECTS
            || count > self.effects.limits.max_intents
            || bytes > self.effects.limits.max_payload_bytes
        {
            return Err(TimelineError::Capacity);
        }
        for effect in self.effects.iter() {
            if step
                .effects
                .iter()
                .any(|new| new.descriptor.identity == effect.descriptor.identity)
            {
                return Err(TimelineError::InputMismatch);
            }
        }
        let head = self.projection.append(step.expected, step.input)?;
        // No user callback/destructor runs until state, head and intents agree.
        let mut vacant = step.effects.slots.iter_mut().filter(|slot| slot.is_none());
        for old in self.effects.slots.iter_mut().filter(|slot| slot.is_some()) {
            if let Some(slot) = vacant.next() {
                *slot = old.take();
            }
        }
        step.effects.len = count;
        step.effects.bytes = bytes;
        step.effects.limits = self.effects.limits;
        let old_state = core::mem::replace(&mut self.state, step.state);
        let old_effects = core::mem::replace(&mut self.effects, step.effects);
        drop(old_state);
        drop(old_effects);
        Ok(head)
    }
    /// Returns only intents at/before the independently effect-finalized frontier.
    pub fn eligible_effects(&self) -> impl Iterator<Item = EligibleEffect<'_, Effect>> {
        let frontier = self.projection.head().effect_finalized();
        self.effects
            .iter()
            .filter(move |effect| {
                frontier.is_some_and(|step| effect.descriptor.origin.step <= step)
            })
            .map(|effect| EligibleEffect {
                descriptor: effect.descriptor,
                payload: &effect.payload,
            })
    }
    /// Durably admits at most `max_intents` eligible effects, retaining ambiguous ones.
    /// Speculative or merely authority-observed steps are never submitted here.
    /// # Errors
    /// Refuses a mismatched receipt or propagates admission/ACK uncertainty. Earlier
    /// confirmed intents remain admitted; every remaining intent retains its identity.
    pub fn handoff<H: EffectHandoff<Effect>>(
        &mut self,
        adapter: &mut H,
        max_intents: usize,
    ) -> Result<usize, HandoffError<H::Error>> {
        let frontier = self.projection.head().effect_finalized();
        let mut admitted = 0usize;
        while admitted < max_intents {
            let index = self
                .effects
                .slots
                .iter()
                .enumerate()
                .filter_map(|(index, slot)| {
                    slot.as_ref()
                        .filter(|effect| {
                            frontier.is_some_and(|step| effect.descriptor.origin.step <= step)
                        })
                        .map(|effect| {
                            (
                                index,
                                effect.descriptor.origin.step,
                                effect.descriptor.identity.digest().0,
                            )
                        })
                })
                .min_by_key(|(_, step, identity)| (*step, *identity))
                .map(|(index, _, _)| index);
            let Some(index) = index else {
                break;
            };
            let slot = self
                .effects
                .slots
                .get_mut(index)
                .ok_or(TimelineError::Capacity)?;
            let effect = slot.as_ref().ok_or(TimelineError::Capacity)?;
            let receipt = adapter
                .submit(EligibleEffect {
                    descriptor: effect.descriptor,
                    payload: &effect.payload,
                })
                .map_err(HandoffError::Store)?;
            if receipt.descriptor != effect.descriptor {
                return Err(TimelineError::InputMismatch.into());
            }
            let len = self
                .effects
                .len
                .checked_sub(1)
                .ok_or(TimelineError::Capacity)?;
            let bytes = self
                .effects
                .bytes
                .checked_sub(effect.descriptor.payload_bytes)
                .ok_or(TimelineError::Capacity)?;
            let retired = slot.take();
            self.effects.len = len;
            self.effects.bytes = bytes;
            admitted = admitted
                .checked_add(1)
                .ok_or(TimelineError::CounterExhausted)?;
            drop(retired);
        }
        Ok(admitted)
    }
}
