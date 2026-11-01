//! Scratch-only replay with resumable bounded calls and consuming validation.
use crate::*;

/// A replay owns isolated scratch and tentative intents, never a mutable live state.
/// Dropping or aborting it only discards the candidate. The caller must separately
/// release its reservation with `TimelineProjection::abort` / `LiveTimeline::abort`.
pub struct ReplaySession<'inputs, State, Effect, const EFFECTS: usize> {
    plan: ReplayPlan<'inputs>,
    reservation: BranchReservation,
    state: State,
    effects: StagedEffects<Effect, EFFECTS>,
    cursor: usize,
    phase: ReplayPhase,
}

/// Validated state is opaque; only successful session validation can construct it.
pub struct ReadyCandidate<State, Effect, const EFFECTS: usize> {
    pub(crate) reservation: BranchReservation,
    pub(crate) replace_from: TimelineStep,
    pub(crate) state: State,
    pub(crate) effects: StagedEffects<Effect, EFFECTS>,
}
impl<'inputs, State, Effect, const EFFECTS: usize> ReplaySession<'inputs, State, Effect, EFFECTS> {
    /// Restores the exact admitted anchor into isolated caller-supplied scratch.
    /// # Errors
    /// Refuses a mismatched reservation or any snapshot restore/identity error.
    pub fn begin<S: SnapshotStore<State>>(
        plan: ReplayPlan<'inputs>,
        reservation: BranchReservation,
        mut scratch: State,
        snapshots: &S,
        limits: EffectLimits,
    ) -> Result<Self, RestoreError<S::Error>> {
        if reservation.expected() != plan.expected_head()
            || reservation.observation().step != plan.corrected_step()
            || reservation.observation().digest != plan.corrected_digest()
        {
            return Err(TimelineError::Conflict.into());
        }
        let restored = snapshots
            .restore(plan.anchor(), &mut scratch)
            .map_err(RestoreError::Snapshot)?;
        if restored != plan.anchor() {
            return Err(TimelineError::InputMismatch.into());
        }
        let phase = if plan.is_empty() {
            ReplayPhase::ReadyToValidate
        } else {
            ReplayPhase::Replaying
        };
        Ok(Self {
            plan,
            reservation,
            state: scratch,
            effects: StagedEffects::new(limits),
            cursor: 0,
            phase,
        })
    }
    /// Current phase; an application panic leaves the phase aborted on unwind.
    #[must_use]
    pub const fn phase(&self) -> ReplayPhase {
        self.phase
    }
    /// Number of successfully replayed steps, independent of wall-clock time.
    #[must_use]
    pub const fn completed_steps(&self) -> usize {
        self.cursor
    }
    /// Read-only scratch for diagnostics, never promoted application state.
    #[must_use]
    pub const fn scratch(&self) -> &State {
        &self.state
    }
    /// Executes at most the caller's budget, preserving exact plan order.
    /// # Errors
    /// Any missing/substituted input, model error, staging error or stale head
    /// aborts the candidate permanently. Live state remains untouched.
    pub fn advance<
        M: RewindableComputation<State = State, Effect = Effect>,
        I: InputStore<M::Input>,
    >(
        &mut self,
        current: TimelineHead,
        model: &M,
        inputs: &I,
        max_steps: u64,
    ) -> Result<ReplayPhase, ReplayError<M::Error, I::Error>> {
        if self.phase == ReplayPhase::Aborted {
            return Err(TimelineError::InvalidPhase.into());
        }
        self.phase = ReplayPhase::Aborted;
        self.plan.check_head(current)?;
        let mut remaining = max_steps;
        while self.cursor < self.plan.len() && remaining > 0 {
            let record = self
                .plan
                .input(self.cursor)
                .ok_or(TimelineError::HistoryUnavailable)?;
            let input = inputs.read(record).map_err(ReplayError::Input)?;
            if input.record != record {
                return Err(TimelineError::InputMismatch.into());
            }
            let context = StepContext {
                revision: current
                    .revision()
                    .checked_next()
                    .ok_or(TimelineError::CounterExhausted)?,
                scope: current.scope(),
                branch: self.reservation.branch(),
                step: record.step,
            };
            let mut sink = self.effects.sink(context);
            model
                .step(&mut self.state, input.value, context, &mut sink)
                .map_err(ReplayError::Computation)?;
            if sink.poisoned {
                return Err(TimelineError::Capacity.into());
            }
            self.cursor = self
                .cursor
                .checked_add(1)
                .ok_or(TimelineError::CounterExhausted)?;
            remaining = remaining
                .checked_sub(1)
                .ok_or(TimelineError::CounterExhausted)?;
        }
        self.phase = if self.cursor == self.plan.len() {
            ReplayPhase::ReadyToValidate
        } else {
            ReplayPhase::Replaying
        };
        Ok(self.phase)
    }
    /// Consumes a complete candidate after application invariant/determinism checks.
    /// # Errors
    /// Refuses incomplete/aborted or stale sessions and failed consumer validation.
    pub fn validate<V: CandidateValidator<State>>(
        self,
        current: TimelineHead,
        validator: &V,
    ) -> Result<ReadyCandidate<State, Effect, EFFECTS>, TimelineError> {
        if self.phase != ReplayPhase::ReadyToValidate {
            return Err(TimelineError::InvalidPhase);
        }
        self.plan.check_head(current)?;
        validator.validate(&self.state, current)?;
        Ok(ReadyCandidate {
            reservation: self.reservation,
            replace_from: self.plan.corrected_step(),
            state: self.state,
            effects: self.effects,
        })
    }
    /// Makes a session permanently unusable, including after a caught consumer panic.
    pub const fn abort(&mut self) {
        self.phase = ReplayPhase::Aborted;
    }
}
