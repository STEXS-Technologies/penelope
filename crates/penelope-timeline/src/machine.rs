//! Opt-in captured machine evaluation over the existing timeline owner.
use crate::{
    EffectSink, HistoryPayload, RewindableComputation, StepContext, TimelineEffectId,
    TimelineError, TimelineScope,
};
use penelope_core::{machine::*, task::Digest};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

/// A complete application-owned machine snapshot. `Clone`, when used by the
/// timeline, must preserve isolation, including any state behind pointers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineTimelineState<State> {
    head: MachineHead,
    state: State,
    last: Option<TransitionRecord>,
}
impl<State> MachineTimelineState<State> {
    pub const fn read(&self) -> (MachineHead, &State) {
        (self.head, &self.state)
    }
    pub const fn last_transition(&self) -> Option<TransitionRecord> {
        self.last
    }
}

/// Captured event and its entire supplied machine context. Extra dependencies
/// (configuration, lookup results, random values) belong in `Event`, not in host
/// services consulted during replay. Neither identity nor digest authenticates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineTimelineInput<Event> {
    pub context: TransitionContext,
    pub event: Event,
}

/// Native machine intent and captured origin, still tentative in the timeline.
/// This is deliberately not a `CommittedIntent`: only the existing timeline
/// finality and handoff protocol can make it eligible for durable admission.
#[derive(Debug)]
pub struct MachineTimelineIntent<Effect> {
    pub context: TransitionContext,
    pub intent: ProposedIntent<Effect>,
}

/// A machine pinned to an explicitly chosen timeline consistency domain.
///
/// This adapter introduces no owner, journal, replay session or dispatch path.
/// Use it with `LiveTimeline`, `prepare_live_step` and existing replay/history
/// adapters. Machine revisions advance independently of timeline revisions and
/// branches. Replayed context is captured input, never reconstructed from the
/// current timeline head. Definition/schema changes require a new replay domain;
/// old captured events cannot be reinterpreted under a different model.
///
/// Eligibility for rewind is an application obligation: the model must be pure,
/// all dependencies captured and snapshots complete and independently owned.
pub struct MachineTimeline<M> {
    model: M,
    machine_scope: MachineScope,
    timeline_scope: TimelineScope,
}
impl<M: Machine> MachineTimeline<M> {
    /// Pins the explicit relationship between distinct machine and timeline roles.
    /// # Errors
    /// Refuses a model inconsistent with the supplied machine definition/schema.
    pub fn new(
        model: M,
        machine_scope: MachineScope,
        timeline_scope: TimelineScope,
    ) -> Result<Self, MachineError> {
        if model.definition() != machine_scope.definition {
            return Err(MachineError::DefinitionMismatch);
        }
        Ok(Self {
            model,
            machine_scope,
            timeline_scope,
        })
    }

    /// Adopts a trusted complete snapshot; does not authenticate or recover it.
    /// # Errors
    /// Refuses incompatible scope/definition or invalid application state.
    pub fn restore(
        &self,
        head: MachineHead,
        state: M::State,
    ) -> Result<MachineTimelineState<M::State>, MachineFailure<M::Error>> {
        self.check_head(head, head)?;
        self.model.validate(&state).map_err(MachineFailure::Model)?;
        Ok(MachineTimelineState {
            head,
            state,
            last: None,
        })
    }

    fn check_head(&self, actual: MachineHead, expected: MachineHead) -> Result<(), MachineError> {
        if actual.scope != self.machine_scope || expected.scope != actual.scope {
            return Err(MachineError::ScopeMismatch);
        }
        if actual.revision != expected.revision {
            return Err(MachineError::StaleHead);
        }
        if self.model.definition() != self.machine_scope.definition {
            return Err(MachineError::DefinitionMismatch);
        }
        Ok(())
    }
}

/// Step failure preserves concrete model and timeline staging errors. Business
/// refusal/unhandled events abort the step; they do not advance either head.
#[derive(Debug, Error)]
pub enum MachineTimelineError<ModelError> {
    #[error(transparent)]
    Machine(#[from] MachineFailure<ModelError>),
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error("machine rejected the event: {reason:?}")]
    Rejected { reason: RejectCode },
    #[error("machine did not handle the event")]
    Unhandled,
}

impl<M: Machine> RewindableComputation for MachineTimeline<M> {
    type State = MachineTimelineState<M::State>;
    type Input = MachineTimelineInput<M::Event>;
    type Effect = MachineTimelineIntent<M::Effect>;
    type Error = MachineTimelineError<M::Error>;

    fn step<S: EffectSink<Effect = Self::Effect>>(
        &self,
        state: &mut Self::State,
        input: &Self::Input,
        context: StepContext,
        effects: &mut S,
    ) -> Result<(), Self::Error> {
        if context.scope != self.timeline_scope {
            return Err(TimelineError::ScopeMismatch.into());
        }
        self.check_head(state.head, input.context.expected)
            .map_err(MachineFailure::Kernel)?;
        // Restored snapshots must satisfy the same complete model invariants as
        // construction. Native timeline restore remains consumer-owned.
        self.model
            .validate(&state.state)
            .map_err(MachineFailure::Model)?;
        let mut sink = MachineEffects {
            effects,
            context: input.context,
            poisoned: None,
            failure: None,
        };
        let decision = self
            .model
            .transition(&state.state, &input.event, &input.context, &mut sink);
        if let Some(error) = sink.failure {
            return Err(error.into());
        }
        sink.status().map_err(MachineFailure::Kernel)?;
        let (next, transition) = match decision.map_err(MachineFailure::Model)? {
            Transition::Accepted { next, transition } => (next, transition),
            Transition::Rejected { reason } => {
                return Err(MachineTimelineError::Rejected { reason });
            }
            Transition::Unhandled => return Err(MachineTimelineError::Unhandled),
        };
        self.model.validate(&next).map_err(MachineFailure::Model)?;
        let revision = state
            .head
            .revision
            .checked_next()
            .ok_or(MachineFailure::Kernel(MachineError::RevisionExhausted))?;
        let old = core::mem::replace(
            state,
            MachineTimelineState {
                head: MachineHead {
                    scope: state.head.scope,
                    revision,
                },
                state: next,
                last: Some(TransitionRecord {
                    context: input.context,
                    revision,
                    transition,
                }),
            },
        );
        // Even a retired application destructor observes a coherent scratch
        // tuple. The real timeline owner still controls live publication.
        drop(old);
        Ok(())
    }
}

struct MachineEffects<'effects, Sink> {
    effects: &'effects mut Sink,
    context: TransitionContext,
    poisoned: Option<MachineError>,
    failure: Option<TimelineError>,
}
impl<Effect, Sink: EffectSink<Effect = MachineTimelineIntent<Effect>>> IntentSink<Effect>
    for MachineEffects<'_, Sink>
{
    fn stage(&mut self, intent: ProposedIntent<Effect>) -> Result<(), MachineError> {
        self.status()?;
        // Pre-poison before transferring a payload: a refused payload's Drop
        // may unwind, and a model may catch that unwind and ignore its error.
        self.poisoned = Some(MachineError::StagingInterrupted);
        let result = self.effects.stage(
            TimelineEffectId::from_digest(Digest(intent.metadata.identity.bytes())),
            Digest(intent.metadata.digest),
            intent.metadata.payload_bytes,
            MachineTimelineIntent {
                context: self.context,
                intent,
            },
        );
        match result {
            Ok(()) => {
                self.poisoned = None;
                Ok(())
            }
            Err(error) => {
                let mapped = if error == TimelineError::InputMismatch {
                    MachineError::DuplicateIntent
                } else {
                    MachineError::Capacity
                };
                self.poisoned = Some(mapped);
                self.failure = Some(error);
                Err(mapped)
            }
        }
    }
    fn status(&self) -> Result<(), MachineError> {
        self.poisoned.map_or(Ok(()), Err)
    }
}

// Versioned content hashing for the owned in-memory history adapters, not a wire
// codec or a durable record format. Payload implementations must bind all bytes.
fn hash_head(hash: &mut Sha256, head: MachineHead) {
    hash.update(head.scope.tenant.bytes());
    hash.update(head.scope.instance.bytes());
    hash.update(head.scope.definition.fingerprint.bytes());
    hash.update(head.scope.definition.state_schema.to_le_bytes());
    hash.update(head.scope.definition.event_schema.to_le_bytes());
    hash.update(head.revision.get().to_le_bytes());
}
fn hash_context(hash: &mut Sha256, context: TransitionContext) {
    hash_head(hash, context.expected);
    hash.update(context.event.bytes());
    hash.update(context.event_digest);
    hash.update(context.logical_time.get().to_le_bytes());
}
impl<State: HistoryPayload> HistoryPayload for MachineTimelineState<State> {
    fn digest(&self) -> Digest {
        let mut hash = Sha256::new();
        hash.update(b"penelope/machine-timeline/state/1");
        hash_head(&mut hash, self.head);
        hash.update(self.state.digest().0);
        hash.update([u8::from(self.last.is_some())]);
        if let Some(record) = self.last {
            hash_context(&mut hash, record.context);
            hash.update(record.revision.get().to_le_bytes());
            hash.update(record.transition.get().to_le_bytes());
        }
        Digest(hash.finalize().into())
    }
    fn byte_len(&self) -> u64 {
        self.state
            .byte_len()
            .saturating_add(if self.last.is_some() { 313 } else { 113 })
    }
}
impl<Event: HistoryPayload> HistoryPayload for MachineTimelineInput<Event> {
    fn digest(&self) -> Digest {
        let mut hash = Sha256::new();
        hash.update(b"penelope/machine-timeline/input/1");
        hash_context(&mut hash, self.context);
        hash.update(self.event.digest().0);
        Digest(hash.finalize().into())
    }
    fn byte_len(&self) -> u64 {
        self.event.byte_len().saturating_add(184)
    }
}
