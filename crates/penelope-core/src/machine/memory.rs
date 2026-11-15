use super::*;

/// Owned bounded intents transferred explicitly from an in-memory publisher.
/// Transfer is not durable admission, dispatch or verified external success.
#[derive(Debug)]
pub struct IntentBatch<Effect, const CAPACITY: usize> {
    slots: [Option<CommittedIntent<Effect>>; CAPACITY],
    len: usize,
    bytes: u64,
}
impl<Effect, const CAPACITY: usize> IntentBatch<Effect, CAPACITY> {
    fn empty() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
            len: 0,
            bytes: 0,
        }
    }
    pub const fn len(&self) -> usize {
        self.len
    }
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub const fn payload_bytes(&self) -> u64 {
        self.bytes
    }
    /// Intents in publication order, then model staging order.
    pub fn iter(&self) -> impl Iterator<Item = &CommittedIntent<Effect>> {
        self.slots.iter().flatten()
    }
}
impl<Effect, const CAPACITY: usize> IntoIterator for IntentBatch<Effect, CAPACITY> {
    type Item = CommittedIntent<Effect>;
    type IntoIter = core::iter::Flatten<core::array::IntoIter<Option<Self::Item>, CAPACITY>>;
    fn into_iter(self) -> Self::IntoIter {
        self.slots.into_iter().flatten()
    }
}

/// Exclusive in-memory owner of the coherent state/head/provenance/intent tuple.
/// There is no mutable state accessor. Model state must not share mutable aliases
/// with inputs, other owners, validators or candidate state. No crash durability,
/// historical event deduplication or external-effect execution is provided.
pub struct MemoryMachine<M: Machine, const CAPACITY: usize> {
    model: M,
    head: MachineHead,
    state: M::State,
    last: Option<TransitionRecord>,
    pending: IntentBatch<M::Effect, CAPACITY>,
    limits: IntentLimits,
}
impl<M: Machine, const CAPACITY: usize> MemoryMachine<M, CAPACITY> {
    /// Creates a revision-zero owner, validating definition, limits and state.
    /// # Errors
    /// Refuses incompatible definitions, impossible limits or invalid initial state.
    pub fn new(
        model: M,
        scope: MachineScope,
        state: M::State,
        limits: IntentLimits,
    ) -> Result<Self, MachineFailure<M::Error>> {
        Self::restore(
            model,
            MachineHead {
                scope,
                revision: MachineRevision::ZERO,
            },
            state,
            limits,
        )
    }

    /// Adopts an application-owned snapshot at an explicit head. The application
    /// must establish snapshot/history authenticity and recover pending intents
    /// separately. This constructor is not a durable recovery implementation.
    /// # Errors
    /// Refuses incompatible definitions, impossible limits or invalid state.
    pub fn restore(
        model: M,
        head: MachineHead,
        state: M::State,
        limits: IntentLimits,
    ) -> Result<Self, MachineFailure<M::Error>> {
        if model.definition() != head.scope.definition {
            return Err(MachineError::DefinitionMismatch.into());
        }
        if limits.max_intents > CAPACITY {
            return Err(MachineError::Capacity.into());
        }
        model.validate(&state).map_err(MachineFailure::Model)?;
        Ok(Self {
            model,
            head,
            state,
            last: None,
            pending: IntentBatch::empty(),
            limits,
        })
    }
    pub const fn read(&self) -> (MachineHead, &M::State) {
        (self.head, &self.state)
    }
    pub const fn last_transition(&self) -> Option<TransitionRecord> {
        self.last
    }
    pub fn pending_intents(&self) -> impl Iterator<Item = &CommittedIntent<M::Effect>> {
        self.pending.iter()
    }
    /// Moves out all pending intents without executing them. The caller owns any
    /// further durability, idempotent admission, dispatch and reconciliation.
    pub fn take_intents(&mut self) -> IntentBatch<M::Effect, CAPACITY> {
        core::mem::replace(&mut self.pending, IntentBatch::empty())
    }

    /// Evaluates and validates without changing any published value. The ready
    /// handle borrows this owner exclusively, so it cannot publish to another
    /// owner, outlive its owner, race another writer or be consumed twice.
    /// # Errors
    /// Refuses scope/head/model mismatch, staging failure, invalid state or overflow.
    pub fn prepare(
        &mut self,
        event: &M::Event,
        context: TransitionContext,
    ) -> Result<PrepareDecision<'_, M, CAPACITY>, MachineFailure<M::Error>> {
        if context.expected.scope != self.head.scope {
            return Err(MachineError::ScopeMismatch.into());
        }
        if context.expected.revision != self.head.revision {
            return Err(MachineError::StaleHead.into());
        }
        if self.model.definition() != self.head.scope.definition {
            return Err(MachineError::DefinitionMismatch.into());
        }
        let mut sink = Staging {
            old: &self.pending,
            slots: core::array::from_fn(|_| None),
            len: self.pending.len,
            bytes: self.pending.bytes,
            limits: self.limits,
            poisoned: None,
        };
        let decision = self
            .model
            .transition(&self.state, event, &context, &mut sink);
        if let Some(error) = sink.poisoned {
            return Err(error.into());
        }
        match decision.map_err(MachineFailure::Model)? {
            Transition::Rejected { reason } => Ok(PrepareDecision::Rejected { reason }),
            Transition::Unhandled => Ok(PrepareDecision::Unhandled),
            Transition::Accepted { next, transition } => {
                self.model.validate(&next).map_err(MachineFailure::Model)?;
                let revision = self
                    .head
                    .revision
                    .checked_next()
                    .ok_or(MachineError::RevisionExhausted)?;
                let record = TransitionRecord {
                    context,
                    revision,
                    transition,
                };
                let mut pending = IntentBatch::empty();
                // Reserve the old prefix without touching it; only tentative
                // new payloads move while constructing the ready handle.
                for (slot, intent) in pending
                    .slots
                    .iter_mut()
                    .skip(self.pending.len)
                    .zip(sink.slots.into_iter().flatten())
                {
                    *slot = Some(CommittedIntent {
                        origin: record,
                        intent,
                    });
                }
                pending.len = sink.len;
                pending.bytes = sink.bytes;
                Ok(PrepareDecision::Ready(PreparedTransition {
                    owner: self,
                    state: next,
                    pending,
                    record,
                }))
            }
        }
    }

    /// Prepares and immediately publishes one admitted local event.
    /// # Errors
    /// Returns preparation failures; rejected/unhandled events change nothing.
    pub fn handle(
        &mut self,
        event: &M::Event,
        context: TransitionContext,
    ) -> Result<HandleOutcome, MachineFailure<M::Error>> {
        Ok(match self.prepare(event, context)? {
            PrepareDecision::Ready(ready) => HandleOutcome::Accepted(ready.publish()),
            PrepareDecision::Rejected { reason } => HandleOutcome::Rejected { reason },
            PrepareDecision::Unhandled => HandleOutcome::Unhandled,
        })
    }
}

/// Preparation distinguishes a publishable candidate from refusal and fallback.
#[allow(clippy::large_enum_variant)] // Caller-sized inline state/intents; boxing would require alloc.
pub enum PrepareDecision<'owner, M: Machine, const CAPACITY: usize> {
    Ready(PreparedTransition<'owner, M, CAPACITY>),
    Rejected { reason: RejectCode },
    Unhandled,
}

/// Opaque, owner-bound candidate. Dropping it aborts the tentative transition.
/// Publishing consumes it and never calls a store or external endpoint.
///
/// ```compile_fail
/// use penelope_core::machine::{Machine, PreparedTransition};
/// fn twice<M: Machine, const N: usize>(ready: PreparedTransition<'_, M, N>) {
///     ready.publish();
///     ready.publish();
/// }
/// ```
/// ```compile_fail
/// use penelope_core::machine::{Machine, MemoryMachine, PrepareDecision, TransitionContext};
/// fn competing<M: Machine, const N: usize>(owner: &mut MemoryMachine<M, N>,
///     event: &M::Event, context: TransitionContext) {
///     if let Ok(PrepareDecision::Ready(ready)) = owner.prepare(event, context) {
///         let _ = owner.read();
///         ready.publish();
///     }
/// }
/// ```
pub struct PreparedTransition<'owner, M: Machine, const CAPACITY: usize> {
    owner: &'owner mut MemoryMachine<M, CAPACITY>,
    state: M::State,
    pending: IntentBatch<M::Effect, CAPACITY>,
    record: TransitionRecord,
}
impl<M: Machine, const CAPACITY: usize> PreparedTransition<'_, M, CAPACITY> {
    pub const fn state(&self) -> &M::State {
        &self.state
    }
    pub const fn record(&self) -> TransitionRecord {
        self.record
    }
    /// Only the newly proposed intents; older pending intents stay in the owner.
    pub fn intents(&self) -> impl Iterator<Item = &ProposedIntent<M::Effect>> {
        self.pending.iter().map(|entry| &entry.intent)
    }
    /// Publishes state, head, provenance and pending intents coherently. No
    /// callback or payload destructor runs between the moves. If a superseded
    /// state's destructor panics, publication has already occurred; inspect the
    /// owner before deciding whether to retry. This is in-memory publication only.
    pub fn publish(mut self) -> TransitionRecord {
        for (old, slot) in self
            .owner
            .pending
            .slots
            .iter_mut()
            .zip(self.pending.slots.iter_mut())
            .take(self.owner.pending.len)
        {
            *slot = old.take();
        }
        let retired_state = core::mem::replace(&mut self.owner.state, self.state);
        let retired_pending = core::mem::replace(&mut self.owner.pending, self.pending);
        self.owner.head.revision = self.record.revision;
        self.owner.last = Some(self.record);
        drop(retired_state);
        drop(retired_pending);
        self.record
    }
}

impl<M: Machine, const CAPACITY: usize> PreparedDecision for PreparedTransition<'_, M, CAPACITY> {
    type State = M::State;
    type Provenance = TransitionRecord;
    type Intent = ProposedIntent<M::Effect>;

    fn before(&self) -> Option<&M::State> {
        Some(&self.owner.state)
    }
    fn candidate(&self) -> &M::State {
        self.state()
    }
    fn provenance(&self) -> &TransitionRecord {
        &self.record
    }
    fn intents(&self) -> impl Iterator<Item = &Self::Intent> {
        self.intents()
    }
}

// len/bytes include the retained prefix; new slots start after that prefix.
struct Staging<'pending, Effect, const CAPACITY: usize> {
    old: &'pending IntentBatch<Effect, CAPACITY>,
    slots: [Option<ProposedIntent<Effect>>; CAPACITY],
    len: usize,
    bytes: u64,
    limits: IntentLimits,
    poisoned: Option<MachineError>,
}
impl<Effect, const CAPACITY: usize> Staging<'_, Effect, CAPACITY> {
    fn admit(&mut self, intent: ProposedIntent<Effect>) -> Result<(), MachineError> {
        let (len, bytes) = crate::__private::admit_intent::<CAPACITY, _, _>(
            self.len,
            self.bytes,
            (self.limits.max_intents, self.limits.max_payload_bytes),
            self.old
                .iter()
                .map(|old| old.intent.metadata.identity)
                .chain(self.slots.iter().flatten().map(|old| old.metadata.identity)),
            intent.metadata.identity,
            intent.metadata.payload_bytes,
            (MachineError::Capacity, MachineError::DuplicateIntent),
        )?;
        let slot = self.slots.get_mut(self.len).ok_or(MachineError::Capacity)?;
        *slot = Some(intent);
        self.len = len;
        self.bytes = bytes;
        Ok(())
    }
}
impl<Effect, const CAPACITY: usize> IntentSink<Effect> for Staging<'_, Effect, CAPACITY> {
    fn status(&self) -> Result<(), MachineError> {
        self.poisoned.map_or(Ok(()), Err)
    }
    fn stage(&mut self, intent: ProposedIntent<Effect>) -> Result<(), MachineError> {
        if let Some(error) = self.poisoned {
            return Err(error);
        }
        let previous = self.poisoned;
        // Set poison before a rejected payload can run arbitrary Drop code.
        self.poisoned = Some(MachineError::StagingInterrupted);
        let result = self.admit(intent);
        self.poisoned = previous.or_else(|| result.as_ref().err().copied());
        result
    }
}
