//! Replay pins retain a shared store borrow across yielding calls.
use crate::*;

/// A session and its immutable input store, kept borrowed until validation/abort.
/// FixedHistory mutation/GC requires an exclusive borrow and cannot race this pin.
///
/// ```compile_fail,E0502
/// use penelope_timeline::{FixedHistory, ReplaySession, TimelineStep};
/// fn cannot_evict(
///     history: &mut FixedHistory<u64, u64, 8, 8, 4>,
///     session: ReplaySession<'_, u64, (), 4>,
/// ) {
///     let pinned = session.pin_inputs(&*history);
///     history.expire_before(TimelineStep::ZERO).unwrap();
///     let _phase = pinned.phase();
/// }
/// ```
pub struct PinnedReplaySession<'store, 'inputs, Store: ?Sized, State, Effect, const EFFECTS: usize>
{
    store: &'store Store,
    session: ReplaySession<'inputs, State, Effect, EFFECTS>,
}
impl<'inputs, State, Effect, const EFFECTS: usize> ReplaySession<'inputs, State, Effect, EFFECTS> {
    /// Retains the input store for all subsequent replay calls.
    #[must_use]
    pub const fn pin_inputs<Store: ?Sized>(
        self,
        store: &Store,
    ) -> PinnedReplaySession<'_, 'inputs, Store, State, Effect, EFFECTS> {
        PinnedReplaySession {
            store,
            session: self,
        }
    }
}
impl<Store: ?Sized, State, Effect, const EFFECTS: usize>
    PinnedReplaySession<'_, '_, Store, State, Effect, EFFECTS>
{
    /// Current candidate phase.
    #[must_use]
    pub const fn phase(&self) -> ReplayPhase {
        self.session.phase()
    }
    /// Read-only isolated scratch for diagnostics or externally staged snapshots.
    #[must_use]
    pub const fn scratch(&self) -> &State {
        self.session.scratch()
    }
    /// Executes a bounded number of steps against the retained immutable store.
    /// # Errors
    /// Returns the same permanent-failure refusals as the underlying session.
    pub fn advance<M: RewindableComputation<State = State, Effect = Effect>>(
        &mut self,
        current: TimelineHead,
        model: &M,
        max_steps: u64,
    ) -> Result<ReplayPhase, ReplayError<M::Error, Store::Error>>
    where
        Store: InputStore<M::Input>,
    {
        self.session.advance(current, model, self.store, max_steps)
    }
    /// Consumes the pin after complete validation; storage can then be maintained.
    /// # Errors
    /// Refuses incomplete, aborted or stale replay and failed consumer invariants.
    pub fn validate<V: CandidateValidator<State>>(
        self,
        current: TimelineHead,
        validator: &V,
    ) -> Result<ReadyCandidate<State, Effect, EFFECTS>, TimelineError> {
        self.session.validate(current, validator)
    }
    /// Aborts scratch; dropping this value releases its history pin.
    pub const fn abort(&mut self) {
        self.session.abort();
    }
}
