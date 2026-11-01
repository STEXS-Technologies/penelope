//! Fixed-size metadata; snapshots, payloads and trust admission belong to the consumer.
use super::*;
use crate::task::Digest;
use thiserror::Error;

/// Immutable consistency domain. This initial API always replays the whole scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineScope {
    pub tenant: TimelineTenantId,
    pub timeline: TimelineId,
    pub model: TimelineModelId,
    pub schema: TimelineSchemaId,
}

/// Accepted state metadata, published atomically with the application state.
///
/// All fields are private: construction checks the finality frontier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineHead {
    pub(super) scope: TimelineScope,
    pub(super) branch: TimelineBranchId,
    pub(super) step: TimelineStep,
    pub(super) revision: TimelineRevision,
    pub(super) effect_finalized: Option<TimelineStep>,
}

impl TimelineHead {
    /// Admits a trusted stored head, including a genesis head at step zero.
    ///
    /// The consumer must authenticate storage and prevent revision/branch reuse.
    /// # Errors
    /// Returns `InvalidFrontier` if finality exceeds the applied step.
    pub fn new(
        scope: TimelineScope,
        branch: TimelineBranchId,
        step: TimelineStep,
        revision: TimelineRevision,
        effect_finalized: Option<TimelineStep>,
    ) -> Result<Self, TimelineError> {
        if effect_finalized.is_some_and(|frontier| frontier.get() > step.get()) {
            return Err(TimelineError::InvalidFrontier);
        }
        Ok(Self {
            scope,
            branch,
            step,
            revision,
            effect_finalized,
        })
    }
    /// Immutable consistency domain.
    #[must_use]
    pub const fn scope(self) -> TimelineScope {
        self.scope
    }
    /// Current branch.
    #[must_use]
    pub const fn branch(self) -> TimelineBranchId {
        self.branch
    }
    /// Latest applied logical input.
    #[must_use]
    pub const fn step(self) -> TimelineStep {
        self.step
    }
    /// Exact expected publication revision.
    #[must_use]
    pub const fn revision(self) -> TimelineRevision {
        self.revision
    }
    /// Irreversible-effect frontier, independently authorized by the application.
    #[must_use]
    pub const fn effect_finalized(self) -> Option<TimelineStep> {
        self.effect_finalized
    }
}

/// Complete state after applying the input at `step`; zero denotes genesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateAnchor {
    pub scope: TimelineScope,
    pub branch: TimelineBranchId,
    pub step: TimelineStep,
    pub snapshot: TimelineSnapshotId,
    pub digest: Digest,
}

/// Immutable payload descriptor. Payload bytes must match this identity at use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputRecord {
    pub scope: TimelineScope,
    pub branch: TimelineBranchId,
    pub step: TimelineStep,
    pub identity: TimelineInputId,
    pub revision: InputRevision,
    pub digest: Digest,
}

/// Trust-admitted correction. Constructing this value does not authenticate it.
/// The application must authorize its source and reject stale authority epochs.
// Fixed-size metadata is intentional: boxing would add allocation to the portable API.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    /// Corrected post-step state; apply inputs strictly after the anchor.
    State(StateAnchor),
    /// Corrected input; restore precisely the state preceding this input.
    Input {
        anchor: StateAnchor,
        replacement: InputRecord,
    },
}

/// Bounds for metadata planning; execution/snapshot byte bounds are separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayLimits {
    pub max_retained_inputs: u64,
    pub max_replay_steps: u64,
}

/// Typed refusals never authorize a partial replacement of live state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TimelineError {
    #[error("scope, model or schema mismatch")]
    ScopeMismatch,
    #[error("stale head revision or branch")]
    Conflict,
    #[error("correction targets a future step")]
    FutureCorrection,
    #[error("required contiguous history or predecessor anchor is unavailable")]
    HistoryUnavailable,
    #[error("correction crosses the irreversible-effect frontier")]
    ReconciliationRequired,
    #[error("configured replay/history capacity exceeded")]
    Capacity,
    #[error("protocol counter exhausted")]
    CounterExhausted,
    #[error("effect-finalized frontier exceeds the live step")]
    InvalidFrontier,
    #[error("input is not the next pinned replay record")]
    InputMismatch,
}
