//! Bounded branch reservations and deterministic head transitions.
//!
//! The projection owns metadata only. A consumer must publish it and application
//! state under the same exclusive ownership / transaction boundary. It must retain
//! the branch high-water mark across recovery; reconstructing from a head alone
//! would permit reuse of abandoned branch identities.
use super::*;

/// Non-reusable branch reservation tied to one exact expected head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchReservation {
    expected: TimelineHead,
    branch: TimelineBranchId,
    observation: AuthorityObservation,
}
impl BranchReservation {
    /// Original head that the candidate may replace.
    #[must_use]
    pub const fn expected(self) -> TimelineHead {
        self.expected
    }
    /// Candidate branch identity, never reused by this projection.
    #[must_use]
    pub const fn branch(self) -> TimelineBranchId {
        self.branch
    }
    /// Exact independently admitted observation bound to the correction.
    #[must_use]
    pub const fn observation(self) -> AuthorityObservation {
        self.observation
    }
    /// Describes the proposed publication for candidate validation. This is not
    /// evidence that the reservation remains active or that publication occurred;
    /// the owning projection still checks both at promotion.
    /// # Errors
    /// Refuses publication revision exhaustion.
    pub fn proposed_head(self) -> Result<TimelineHead, TimelineError> {
        let mut head = self.expected;
        head.branch = self.branch;
        head.revision = head
            .revision
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        head.authority = Some(self.observation);
        Ok(head)
    }
}

/// Allocation-free metadata controller with a compile-time concurrent-session bound.
///
/// Not `Clone`: callers cannot accidentally rewind its branch high-water mark.
#[derive(Debug)]
pub struct TimelineProjection<const SESSIONS: usize> {
    head: TimelineHead,
    last_branch: TimelineBranchId,
    sessions: [Option<BranchReservation>; SESSIONS],
}
impl<const SESSIONS: usize> TimelineProjection<SESSIONS> {
    /// Starts an in-memory timeline with no abandoned branch identities.
    #[must_use]
    pub const fn new(head: TimelineHead) -> Self {
        Self {
            head,
            last_branch: head.branch(),
            sessions: [None; SESSIONS],
        }
    }
    /// Restores a trusted projection including its persistent branch high-water mark.
    /// Active sessions are not resumed; their reserved identities remain consumed.
    /// # Errors
    /// Refuses a high-water mark preceding the current branch.
    pub fn restore(
        head: TimelineHead,
        last_branch: TimelineBranchId,
    ) -> Result<Self, TimelineError> {
        if last_branch < head.branch() {
            return Err(TimelineError::Conflict);
        }
        Ok(Self {
            head,
            last_branch,
            sessions: [None; SESSIONS],
        })
    }
    /// Current immutable head metadata.
    #[must_use]
    pub const fn head(&self) -> TimelineHead {
        self.head
    }
    /// High-water mark that must be retained when saving metadata.
    #[must_use]
    pub const fn last_branch(&self) -> TimelineBranchId {
        self.last_branch
    }
    /// Number of outstanding reservations, including stale candidates awaiting abort.
    #[must_use]
    pub fn active_sessions(&self) -> usize {
        self.sessions.iter().flatten().count()
    }
    /// Reserves a fresh immutable child branch without modifying live head state.
    /// The application must authenticate the observation and full consistency domain.
    /// # Errors
    /// Refuses stale heads/authority, incorrect observation bindings, exhausted
    /// branch counters and concurrent-session capacity.
    pub fn reserve(
        &mut self,
        plan: &ReplayPlan<'_>,
        observation: AuthorityObservation,
    ) -> Result<BranchReservation, TimelineError> {
        plan.check_head(self.head)?;
        // An observation may have been admitted before its replay is prepared.
        // Reusing that exact accepted binding is not an out-of-order observation.
        if self.head.authority != Some(observation) {
            self.check_observation(observation)?;
        }
        if observation.step != plan.corrected_step()
            || observation.digest != plan.corrected_digest()
        {
            return Err(TimelineError::InputMismatch);
        }
        let slot = self
            .sessions
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(TimelineError::Capacity)?;
        let branch = self
            .last_branch
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        let reservation = BranchReservation {
            expected: self.head,
            branch,
            observation,
        };
        *slot = Some(reservation);
        self.last_branch = branch;
        Ok(reservation)
    }
    /// Checks a candidate and returns its proposed head without publishing it.
    /// Actual state validation belongs to the replay facade/consumer.
    /// # Errors
    /// Refuses unknown, aborted, reused or stale reservations and exhausted epochs.
    pub fn promotion_head(
        &self,
        reservation: BranchReservation,
    ) -> Result<TimelineHead, TimelineError> {
        self.check_reservation(reservation)?;
        reservation.proposed_head()
    }
    /// Publishes validated candidate metadata within the consumer's state transaction.
    /// Do not call separately from the application state swap for shared readers.
    /// # Errors
    /// Returns the same typed refusals as `promotion_head`; no mutation on failure.
    pub fn promote(
        &mut self,
        reservation: BranchReservation,
    ) -> Result<TimelineHead, TimelineError> {
        let head = self.promotion_head(reservation)?;
        self.abort(reservation)?;
        self.head = head;
        Ok(head)
    }
    /// Releases a reservation, including a stale one; its identity stays consumed.
    /// # Errors
    /// Refuses an unknown or already released reservation.
    pub fn abort(&mut self, reservation: BranchReservation) -> Result<(), TimelineError> {
        let slot = self
            .sessions
            .iter_mut()
            .find(|slot| **slot == Some(reservation))
            .ok_or(TimelineError::SessionUnavailable)?;
        *slot = None;
        Ok(())
    }
    /// Advances a live step after successful scratch execution and atomic publication.
    /// # Errors
    /// Refuses stale head, scope/branch substitution, a gap, or counter exhaustion.
    pub fn append(
        &mut self,
        expected: TimelineHead,
        input: InputRecord,
    ) -> Result<TimelineHead, TimelineError> {
        self.check_head(expected)?;
        if input.scope != self.head.scope {
            return Err(TimelineError::ScopeMismatch);
        }
        if input.branch != self.head.branch {
            return Err(TimelineError::Conflict);
        }
        let step = self
            .head
            .step
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        if input.step != step {
            return Err(TimelineError::InputMismatch);
        }
        let revision = self
            .head
            .revision
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        self.head.step = step;
        self.head.revision = revision;
        Ok(self.head)
    }
    /// Records an authenticated observation without finalizing any effects.
    /// # Errors
    /// Refuses stale authority/head, future observations, or counter exhaustion.
    pub fn observe(
        &mut self,
        expected: TimelineHead,
        observation: AuthorityObservation,
    ) -> Result<TimelineHead, TimelineError> {
        self.check_head(expected)?;
        self.check_observation(observation)?;
        let revision = self
            .head
            .revision
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        self.head.authority = Some(observation);
        self.head.revision = revision;
        Ok(self.head)
    }
    /// Independently finalizes an irreversible-effect prefix after application proof admission.
    /// Never interprets authority epoch advancement as permission to reverse effects.
    /// # Errors
    /// Refuses a stale head, backward/future frontier or exhausted revision.
    pub fn finalize_effects(
        &mut self,
        expected: TimelineHead,
        through: TimelineStep,
    ) -> Result<TimelineHead, TimelineError> {
        self.check_head(expected)?;
        if through > self.head.step || self.head.effect_finalized.is_some_and(|old| through <= old)
        {
            return Err(TimelineError::InvalidFrontier);
        }
        let revision = self
            .head
            .revision
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        self.head.effect_finalized = Some(through);
        self.head.revision = revision;
        Ok(self.head)
    }
    fn check_head(&self, expected: TimelineHead) -> Result<(), TimelineError> {
        if expected != self.head {
            return Err(TimelineError::Conflict);
        }
        Ok(())
    }
    fn check_reservation(&self, reservation: BranchReservation) -> Result<(), TimelineError> {
        if !self.sessions.contains(&Some(reservation)) {
            return Err(TimelineError::SessionUnavailable);
        }
        self.check_head(reservation.expected)
    }
    fn check_observation(&self, observation: AuthorityObservation) -> Result<(), TimelineError> {
        if observation.scope != self.head.scope {
            return Err(TimelineError::ScopeMismatch);
        }
        if observation.step > self.head.step {
            return Err(TimelineError::FutureCorrection);
        }
        if self.head.authority.is_some_and(|old| {
            observation.authority != old.authority
                || (observation.epoch, observation.sequence) <= (old.epoch, old.sequence)
        }) {
            return Err(TimelineError::StaleAuthority);
        }
        Ok(())
    }
}
