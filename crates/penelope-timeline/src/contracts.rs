//! Consumer contracts. Determinism, complete snapshots and trust are explicit obligations.
use crate::{
    EffectSink, InputRecord, StateAnchor, TimelineBranchId, TimelineHead, TimelineRevision,
    TimelineScope, TimelineStep,
};
use penelope_core::{task::Digest, timeline::TimelineError};
use thiserror::Error;

/// Context captured by the timeline; models must not consult ambient clocks or RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepContext {
    pub revision: TimelineRevision,
    pub scope: TimelineScope,
    pub branch: TimelineBranchId,
    pub step: TimelineStep,
}

/// Replay-safe computation. `step` mutates only isolated scratch state and the sink.
/// It must not perform irreversible I/O or read uncaptured nondeterminism. Interior
/// aliases to exposed live state violate the snapshot/isolation contract.
pub trait RewindableComputation {
    type State;
    type Input;
    type Effect;
    type Error;
    /// Executes exactly one logical input.
    /// # Errors
    /// Returns consumer failures; any error permanently aborts the session.
    fn step<S: EffectSink<Effect = Self::Effect>>(
        &self,
        state: &mut Self::State,
        input: &Self::Input,
        context: StepContext,
        effects: &mut S,
    ) -> Result<(), Self::Error>;
}

/// Snapshot adapter. Restore must produce a complete isolated state, verify its
/// model/schema and configured integrity/size limits before allocating/copying.
/// Snapshot handles and any delta bases must stay pinned for the session lifetime.
pub trait SnapshotStore<State> {
    type Error;
    /// Restores the requested immutable anchor into caller-supplied scratch.
    /// # Errors
    /// Rejects unavailable, incompatible, corrupt or over-budget snapshots.
    fn restore(&self, anchor: StateAnchor, scratch: &mut State)
    -> Result<StateAnchor, Self::Error>;
}

/// Snapshot adapter that additionally binds the restore to the canonical live head.
/// A correction import at the same step is not automatically the accepted live state.
pub trait LiveSnapshotStore<State>: SnapshotStore<State> {
    /// Restores the exact state published with `expected`; refuses tentative imports.
    /// # Errors
    /// Rejects an anchor that is not canonical for that head, or any restore failure.
    fn restore_current(
        &self,
        expected: TimelineHead,
        anchor: StateAnchor,
        scratch: &mut State,
    ) -> Result<StateAnchor, Self::Error>;
}

/// Immutable verified input access. Implementations must verify bytes/content
/// against the descriptor before returning; a digest alone does not admit trust.
pub trait InputStore<Input> {
    type Error;
    /// Reads the exact version, scope and payload identity requested.
    /// # Errors
    /// Refuses missing, substituted, corrupt or over-budget input payloads.
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, Input>, Self::Error>;
}

/// Consumer-verified payload with its exact immutable descriptor.
pub struct BoundInput<'input, Input> {
    pub record: InputRecord,
    pub value: &'input Input,
}

/// Required candidate invariant validation. Optional digest comparison must use
/// the same corrected baseline/inputs, never the old intentionally corrected state.
pub trait CandidateValidator<State> {
    /// Checks complete domain invariants and optionally deterministic state identity.
    /// # Errors
    /// Rejects any unverifiable or invalid state; a rejection cannot be retried as success.
    fn validate(&self, state: &State, expected: TimelineHead) -> Result<(), TimelineError>;
}

/// Optional same-baseline digest checker; no mandatory state hash on each step.
pub struct DigestValidator<F> {
    pub expected: Digest,
    pub digest: F,
}
impl<State, F: Fn(&State) -> Digest> CandidateValidator<State> for DigestValidator<F> {
    fn validate(&self, state: &State, _expected: TimelineHead) -> Result<(), TimelineError> {
        if (self.digest)(state) != self.expected {
            return Err(TimelineError::ValidationFailed);
        }
        Ok(())
    }
}

/// Replay execution status; ready still requires consumer validation and CAS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayPhase {
    Replaying,
    ReadyToValidate,
    Aborted,
}

/// Independent bounds for tentative effect count (also compile-time bounded) and bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectLimits {
    pub max_intents: usize,
    pub max_payload_bytes: u64,
}

/// Consumer errors retain their concrete types without becoming success evidence.
#[derive(Debug, Error)]
pub enum ReplayError<ModelError, StoreError> {
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error("consumer computation failed")]
    Computation(ModelError),
    #[error("consumer input access failed")]
    Input(StoreError),
}

/// Snapshot restoration errors are independent from computation/input access errors.
#[derive(Debug, Error)]
pub enum RestoreError<StoreError> {
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error("consumer snapshot restoration failed")]
    Snapshot(StoreError),
}
