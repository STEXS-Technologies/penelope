use super::*;
use thiserror::Error;

/// Pinned interpretation of machine state and events. Digests are not authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineDefinition {
    pub fingerprint: MachineDefinitionId,
    pub state_schema: u32,
    pub event_schema: u32,
}

/// Exact tenant, instance and pinned interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineScope {
    pub tenant: MachineTenantId,
    pub instance: MachineId,
    pub definition: MachineDefinition,
}

/// A complete publication coordinate. Does not authenticate a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineHead {
    pub scope: MachineScope,
    pub revision: MachineRevision,
}

/// Entire supplied logical context. No clock, random source or host services.
/// Event digest/identity are caller assertions in the in-memory profile; a
/// durable codec/journal must independently bind them to immutable event bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionContext {
    pub expected: MachineHead,
    pub event: MachineEventId,
    pub event_digest: [u8; 32],
    pub logical_time: MachineLogicalTime,
}

/// A model's decision, not a publication or authorization receipt.
#[derive(Debug, PartialEq, Eq)]
pub enum Transition<State> {
    Accepted {
        next: State,
        transition: TransitionId,
    },
    Rejected {
        reason: RejectCode,
    },
    Unhandled,
}

/// Count and charged payload limits, independent of application allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntentLimits {
    pub max_intents: usize,
    pub max_payload_bytes: u64,
}

/// Caller-defined immutable intent coordinates. The charge is not a measurement
/// of heap use, and a supplied digest is not an authentication proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntentMetadata {
    pub identity: MachineIntentId,
    pub digest: [u8; 32],
    pub payload_bytes: u64,
}

/// A proposed payload, never dispatched by the transition evaluator.
#[derive(Debug)]
pub struct ProposedIntent<Effect> {
    pub metadata: IntentMetadata,
    pub payload: Effect,
}

/// Bounded staging interface. The owning preparation poisons on any admission
/// failure, even when a model ignores the returned error or catches an unwind.
pub trait IntentSink<Effect> {
    /// Stages an intent without publishing or executing it.
    /// # Errors
    /// Refuses duplicate identities, count/byte exhaustion or poisoned staging.
    fn stage(&mut self, intent: ProposedIntent<Effect>) -> Result<(), MachineError>;
    /// Checks whether any staging attempt failed or was interrupted. Hierarchy
    /// adapters check this between hooks so ignored errors stop further hooks.
    /// # Errors
    /// Returns the first failed admission or an interrupted staging marker.
    fn status(&self) -> Result<(), MachineError>;
}

/// Runtime-neutral pure model. State-local data belongs in ordinary Rust enums.
/// No Clone, serialization, Send, Sync, std or allocator bound is imposed.
pub trait Machine {
    type State;
    type Event;
    type Effect;
    type Error;

    fn definition(&self) -> MachineDefinition;

    /// Evaluates against immutable published state and supplied logical context.
    /// # Errors
    /// Returns a concrete application evaluation failure; no publication occurs.
    fn transition<S: IntentSink<Self::Effect>>(
        &self,
        current: &Self::State,
        event: &Self::Event,
        context: &TransitionContext,
        intents: &mut S,
    ) -> Result<Transition<Self::State>, Self::Error>;

    /// Checks complete state invariants at construction and before publication.
    /// # Errors
    /// Returns a concrete application invariant failure.
    fn validate(&self, state: &Self::State) -> Result<(), Self::Error>;
}

/// Proposed or accepted transition coordinates, not a persisted event log.
/// Only an owner publication establishes acceptance; this value alone does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionRecord {
    pub context: TransitionContext,
    pub revision: MachineRevision,
    pub transition: TransitionId,
}

/// An intent retained by the owner after its originating publication.
/// This DTO alone is not a durable admission or external authorization proof.
#[derive(Debug)]
pub struct CommittedIntent<Effect> {
    pub origin: TransitionRecord,
    pub intent: ProposedIntent<Effect>,
}

/// Final disposition of an in-memory handle call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleOutcome {
    Accepted(TransitionRecord),
    Rejected { reason: RejectCode },
    Unhandled,
}

/// Portable publication failures, separate from business refusal and model faults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MachineError {
    #[error("machine scope does not match")]
    ScopeMismatch,
    #[error("machine head is stale")]
    StaleHead,
    #[error("model definition does not match the pinned scope")]
    DefinitionMismatch,
    #[error("intent capacity or payload charge exhausted")]
    Capacity,
    #[error("intent identity is already pending")]
    DuplicateIntent,
    #[error("intent staging was interrupted")]
    StagingInterrupted,
    #[error("hierarchy path repeats a node or violates the selected target")]
    InvalidHierarchy,
    #[error("hierarchy path exceeds its configured depth")]
    HierarchyDepth,
    #[error("machine revision exhausted")]
    RevisionExhausted,
}

/// Infrastructure and model failures retain distinct concrete types.
#[derive(Debug, Error)]
pub enum MachineFailure<ModelError> {
    #[error(transparent)]
    Kernel(#[from] MachineError),
    #[error("machine evaluation or invariant validation failed")]
    Model(ModelError),
}
