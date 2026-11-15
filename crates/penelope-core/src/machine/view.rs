//! Common inspection without a common publication authority.

/// Read-only inspection of an evaluated, still-unpublished decision.
///
/// Associated types retain each engine's state, provenance and intent semantics.
/// This view supplies no authorization, durable receipt, dispatch or generic
/// publication operation. A legacy protocol may omit coordinates; adapters must
/// preserve that absence rather than inventing machine revisions or identities.
pub trait PreparedDecision {
    type State;
    type Provenance;
    type Intent;

    /// Predecessor used for evaluation, when available. None does not prove that
    /// the instance is absent from an authoritative store.
    fn before(&self) -> Option<&Self::State>;
    fn candidate(&self) -> &Self::State;
    /// Proposed source coordinates, not evidence of durable admission.
    fn provenance(&self) -> &Self::Provenance;
    /// Planned intents, never proof of dispatch or external success.
    fn intents(&self) -> impl Iterator<Item = &Self::Intent>;
}
