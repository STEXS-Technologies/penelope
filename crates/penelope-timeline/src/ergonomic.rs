//! Convenience operations over the same checked publication boundary.
use crate::*;
use thiserror::Error;

/// Owned-state application failure, preserving the model's concrete error.
#[derive(Debug, Error)]
pub enum ApplyError<ModelError> {
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error("consumer computation failed")]
    Computation(ModelError),
}

/// Inputs to a scoped correction. Records and snapshots must already be admitted
/// and retained by the application. No history or authority is inferred here.
pub struct CorrectionRequest<'inputs, State> {
    pub correction: Correction,
    pub inputs: &'inputs [InputRecord],
    pub observation: AuthorityObservation,
    pub scratch: State,
    pub limits: ReplayLimits,
}

/// Exclusive, bounded correction with automatic reservation cleanup, including
/// unwind. Dropping it discards scratch. Only `publish` can replace live state.
/// The exclusive borrow intentionally prevents live appends during this profile;
/// use the lower-level sessions for concurrent competing publications.
///
/// ```compile_fail,E0502
/// use penelope_timeline::*;
/// fn exclusive(
///     live: &mut LiveTimeline<u64, (), 1, 0>,
///     request: CorrectionRequest<'_, u64>,
///     snapshot: &CapturedSnapshot<u64>,
/// ) {
///     let correction = live.begin_correction(request, snapshot).unwrap();
///     let _head = live.read(); // the correction still owns the exclusive borrow
///     let _unchanged = correction.read();
/// }
/// ```
#[must_use = "advance and publish the correction, or drop it to cancel"]
pub struct ScopedCorrection<
    'live,
    'inputs,
    State,
    Effect,
    const SESSIONS: usize,
    const EFFECTS: usize,
> {
    live: &'live mut LiveTimeline<State, Effect, SESSIONS, EFFECTS>,
    reservation: Option<BranchReservation>,
    session: Option<ReplaySession<'inputs, State, Effect, EFFECTS>>,
}

impl<State, Effect, const SESSIONS: usize, const EFFECTS: usize>
    LiveTimeline<State, Effect, SESSIONS, EFFECTS>
{
    /// Constructs the next descriptor from the current head and captured payload.
    /// The caller supplies a stable identity; hashing does not authenticate input.
    /// This does not admit or retain the payload in a history store.
    /// # Errors
    /// Refuses logical-step exhaustion.
    pub fn next_input<Input: HistoryPayload>(
        &self,
        identity: TimelineInputId,
        input: &Input,
    ) -> Result<InputRecord, TimelineError> {
        let head = self.read().0;
        Ok(InputRecord {
            scope: head.scope(),
            branch: head.branch(),
            step: head
                .step()
                .checked_next()
                .ok_or(TimelineError::CounterExhausted)?,
            identity,
            revision: InputRevision::ZERO,
            digest: input.digest(),
        })
    }

    /// Computes a new input on a complete isolated clone of owned live state,
    /// validates it and atomically publishes state, metadata and tentative effects.
    /// No snapshot/input adapter is needed. `Clone` must preserve all state and
    /// must not retain mutable aliases. The returned record can be retained by a
    /// consumer history adapter; this operation itself does not retain history.
    /// # Errors
    /// Refuses computation, validation, counter, duplicate-intent or quota failure;
    /// an error leaves the live publication unchanged.
    pub fn apply_input<M, V>(
        &mut self,
        model: &M,
        identity: TimelineInputId,
        input: &M::Input,
        validator: &V,
    ) -> Result<InputRecord, ApplyError<M::Error>>
    where
        State: Clone,
        M: RewindableComputation<State = State, Effect = Effect>,
        M::Input: HistoryPayload,
        V: CandidateValidator<State>,
    {
        let record = self.next_input(identity, input)?;
        let expected = self.read().0;
        let mut check = TimelineProjection::<0>::new(expected);
        check.append(expected, record)?;
        let mut state = self.read().1.clone();
        let mut effects = StagedEffects::new(self.effect_limits());
        let context = StepContext {
            revision: check.head().revision(),
            scope: expected.scope(),
            branch: expected.branch(),
            step: record.step,
        };
        let mut sink = effects.sink(context);
        model
            .step(&mut state, input, context, &mut sink)
            .map_err(ApplyError::Computation)?;
        if sink.poisoned {
            return Err(TimelineError::Capacity.into());
        }
        validator.validate(&state, check.head())?;
        self.append(ReadyStep {
            expected,
            input: record,
            state,
            effects,
        })?;
        Ok(record)
    }

    /// Plans, reserves and restores a scoped correction. Any restore error or
    /// panic releases the reserved slot automatically. Snapshot and input history
    /// remain application-owned; this method does not create a FixedHistory fork.
    /// # Errors
    /// Refuses invalid plans/authority, unavailable reservations or restore failure.
    pub fn begin_correction<'live, 'inputs, S: SnapshotStore<State>>(
        &'live mut self,
        request: CorrectionRequest<'inputs, State>,
        snapshots: &S,
    ) -> Result<
        ScopedCorrection<'live, 'inputs, State, Effect, SESSIONS, EFFECTS>,
        RestoreError<S::Error>,
    > {
        let head = self.read().0;
        let plan = ReplayPlan::prepare(
            head,
            head.revision(),
            request.correction,
            request.inputs,
            request.limits,
        )?;
        let reservation = self.reserve(&plan, request.observation)?;
        // Install cleanup before invoking consumer restoration (which can unwind).
        let mut scoped = ScopedCorrection {
            live: self,
            reservation: Some(reservation),
            session: None,
        };
        scoped.session = Some(ReplaySession::begin(
            plan,
            reservation,
            request.scratch,
            snapshots,
            scoped.live.effect_limits(),
        )?);
        Ok(scoped)
    }
}

impl<State, Effect, const SESSIONS: usize, const EFFECTS: usize>
    ScopedCorrection<'_, '_, State, Effect, SESSIONS, EFFECTS>
{
    /// Reads the unchanged live publication while this correction is in progress.
    pub const fn read(&self) -> (TimelineHead, &State) {
        self.live.read()
    }

    /// Replays at most `max_steps`, without repeating metadata or reservation work.
    /// # Errors
    /// Refuses aborted sessions, missing input, model failure or effect overflow.
    pub fn advance<M, I>(
        &mut self,
        model: &M,
        inputs: &I,
        max_steps: u64,
    ) -> Result<ReplayPhase, ReplayError<M::Error, I::Error>>
    where
        M: RewindableComputation<State = State, Effect = Effect>,
        I: InputStore<M::Input>,
    {
        self.session
            .as_mut()
            .ok_or(TimelineError::SessionUnavailable)?
            .advance(self.live.read().0, model, inputs, max_steps)
    }

    /// Validates and publishes a completed correction in one consuming operation.
    /// Errors and unwinds release the reservation; cancellation requires only drop.
    /// # Errors
    /// Refuses incomplete/aborted replay, failed invariants or publication quotas.
    pub fn publish<V: CandidateValidator<State>>(
        mut self,
        validator: &V,
    ) -> Result<TimelineHead, TimelineError> {
        let session = self
            .session
            .take()
            .ok_or(TimelineError::SessionUnavailable)?;
        let candidate = session.validate(self.live.read().0, validator)?;
        let head = self.live.promote(candidate)?;
        self.reservation = None;
        Ok(head)
    }
}

impl<State, Effect, const SESSIONS: usize, const EFFECTS: usize> Drop
    for ScopedCorrection<'_, '_, State, Effect, SESSIONS, EFFECTS>
{
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            // Publication may have completed before a retired payload's Drop
            // panicked. In that case the token is already consumed; never panic
            // again during cleanup.
            let _cleanup = self.live.abort(reservation);
        }
    }
}
