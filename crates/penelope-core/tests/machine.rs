//! Independent transition tables, retained intents, failure and unwind boundaries.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::panic_in_result_fn,
    clippy::shadow_unrelated
)]
use penelope_core::machine::*;
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

const fn definition() -> MachineDefinition {
    MachineDefinition {
        fingerprint: MachineDefinitionId::from_bytes([3; 32]),
        state_schema: 1,
        event_schema: 2,
    }
}
const fn scope() -> MachineScope {
    MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: definition(),
    }
}
const fn limits(count: usize, bytes: u64) -> IntentLimits {
    IntentLimits {
        max_intents: count,
        max_payload_bytes: bytes,
    }
}
fn context(head: MachineHead, event: u64) -> TransitionContext {
    let mut bytes = [0; 32];
    bytes
        .get_mut(..8)
        .unwrap()
        .copy_from_slice(&event.to_be_bytes());
    TransitionContext {
        expected: head,
        event: MachineEventId::from_bytes(bytes),
        event_digest: bytes,
        logical_time: MachineLogicalTime::new(event),
    }
}
const fn metadata(context: &TransitionContext, bytes: u64) -> IntentMetadata {
    IntentMetadata {
        identity: MachineIntentId::from_bytes(context.event.bytes()),
        digest: context.event_digest,
        payload_bytes: bytes,
    }
}
// Deliberately neither Clone nor Default, and no serialization implementation.
#[derive(Debug, PartialEq, Eq)]
struct State {
    value: u64,
}
#[derive(Debug, PartialEq, Eq)]
enum Fault {
    Invalid,
    Arithmetic,
    Evaluation,
}
enum Event {
    Add(u64),
    Denied,
    Unknown,
    Fault,
    Poison,
    Overflow,
    Panic,
}
struct Counter;
impl Machine for Counter {
    type State = State;
    type Event = Event;
    type Effect = u64;
    type Error = Fault;
    fn definition(&self) -> MachineDefinition {
        definition()
    }
    fn validate(&self, state: &State) -> Result<(), Fault> {
        if state.value > 100 {
            Err(Fault::Invalid)
        } else {
            Ok(())
        }
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &State,
        event: &Event,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<State>, Fault> {
        let _ignored = sink.stage(ProposedIntent {
            metadata: metadata(context, 8),
            payload: current.value,
        });
        match event {
            Event::Denied => {
                return Ok(Transition::Rejected {
                    reason: RejectCode::new(7),
                });
            }
            Event::Unknown => return Ok(Transition::Unhandled),
            Event::Fault => return Err(Fault::Evaluation),
            Event::Panic => panic!("model panic after staging"),
            Event::Poison => {
                let _ignored = sink.stage(ProposedIntent {
                    metadata: metadata(context, 8),
                    payload: 999,
                });
            }
            Event::Overflow => {
                let _ignored = sink.stage(ProposedIntent {
                    metadata: IntentMetadata {
                        identity: MachineIntentId::from_bytes([99; 32]),
                        payload_bytes: u64::MAX,
                        ..metadata(context, 0)
                    },
                    payload: 999,
                });
            }
            Event::Add(_) => {}
        }
        let amount = if let Event::Add(amount) = event {
            *amount
        } else {
            1
        };
        let value = current.value.checked_add(amount).ok_or(Fault::Arithmetic)?;
        Ok(Transition::Accepted {
            next: State { value },
            transition: TransitionId::new(11),
        })
    }
}
fn owner<const N: usize>() -> MemoryMachine<Counter, N> {
    MemoryMachine::new(Counter, scope(), State { value: 0 }, limits(N, u64::MAX)).unwrap()
}

#[test]
fn decisions_prepare_without_publishing_and_drop_aborts() {
    let mut machine = owner::<4>();
    let head = machine.read().0;
    if let PrepareDecision::Ready(ready) =
        machine.prepare(&Event::Add(4), context(head, 1)).unwrap()
    {
        assert_eq!(ready.state().value, 4);
        assert_eq!(ready.record().revision, MachineRevision::new(1));
        assert_eq!(ready.intents().count(), 1);
        // Leaving this scope aborts the candidate without publication.
    } else {
        panic!("expected candidate");
    }
    assert_eq!(machine.read(), (head, &State { value: 0 }));
    assert!(machine.last_transition().is_none());
    assert_eq!(machine.pending_intents().count(), 0);
    assert_eq!(
        machine.handle(&Event::Denied, context(head, 2)).unwrap(),
        HandleOutcome::Rejected {
            reason: RejectCode::new(7)
        }
    );
    assert_eq!(
        machine.handle(&Event::Unknown, context(head, 3)).unwrap(),
        HandleOutcome::Unhandled
    );
    assert_eq!(machine.read().0, head);
    assert_eq!(machine.pending_intents().count(), 0);
}

#[test]
fn publication_retains_prior_intents_and_their_exact_origins_until_taken() {
    let mut machine = owner::<4>();
    for event in 1..=3 {
        let context = context(machine.read().0, event);
        let HandleOutcome::Accepted(record) = machine.handle(&Event::Add(event), context).unwrap()
        else {
            panic!("expected acceptance");
        };
        assert_eq!(record.context, context);
        assert_eq!(record.revision.get(), event);
        assert_eq!(machine.last_transition(), Some(record));
    }
    let batch = machine.take_intents();
    assert_eq!(batch.len(), 3);
    assert_eq!(batch.payload_bytes(), 24);
    assert_eq!(
        batch
            .iter()
            .map(|intent| (intent.origin.revision.get(), intent.intent.payload))
            .collect::<Vec<_>>(),
        vec![(1, 0), (2, 1), (3, 3)]
    );
    assert_eq!(batch.into_iter().count(), 3);
    assert_eq!(machine.pending_intents().count(), 0);
    assert_eq!(machine.read().1.value, 6);
    assert_eq!(machine.take_intents().len(), 0);
}

#[test]
fn ignored_staging_errors_validation_and_panics_preserve_the_entire_publication() {
    let mut machine = owner::<4>();
    let head = machine.read().0;
    machine.handle(&Event::Add(5), context(head, 1)).unwrap();
    let head = machine.read().0;
    let last = machine.last_transition();
    for (event, expected) in [
        (Event::Poison, MachineError::DuplicateIntent),
        (Event::Overflow, MachineError::Capacity),
    ] {
        assert!(
            matches!(machine.handle(&event, context(head, 2)), Err(MachineFailure::Kernel(error)) if error == expected)
        );
    }
    assert!(matches!(
        machine.handle(&Event::Add(100), context(head, 2)),
        Err(MachineFailure::Model(Fault::Invalid))
    ));
    assert!(matches!(
        machine.handle(&Event::Fault, context(head, 2)),
        Err(MachineFailure::Model(Fault::Evaluation))
    ));
    assert!(
        catch_unwind(AssertUnwindSafe(
            || machine.handle(&Event::Panic, context(head, 2))
        ))
        .is_err()
    );
    assert_eq!(machine.read(), (head, &State { value: 5 }));
    assert_eq!(machine.last_transition(), last);
    assert_eq!(
        machine
            .pending_intents()
            .map(|intent| intent.origin.revision.get())
            .collect::<Vec<_>>(),
        vec![1]
    );
}

#[test]
fn scope_model_stale_head_pending_duplicates_and_budgets_fail_closed() {
    assert!(matches!(
        MemoryMachine::<Counter, 1>::new(Counter, scope(), State { value: 0 }, limits(2, 8)),
        Err(MachineFailure::Kernel(MachineError::Capacity))
    ));
    let mut wrong_scope = scope();
    wrong_scope.definition.event_schema = 8;
    assert!(matches!(
        MemoryMachine::<Counter, 1>::new(Counter, wrong_scope, State { value: 0 }, limits(1, 8)),
        Err(MachineFailure::Kernel(MachineError::DefinitionMismatch))
    ));
    let mut machine = owner::<2>();
    let initial = machine.read().0;
    let mut wrong = context(initial, 1);
    wrong.expected.scope.tenant = MachineTenantId::from_bytes([99; 32]);
    assert!(matches!(
        machine.handle(&Event::Add(1), wrong),
        Err(MachineFailure::Kernel(MachineError::ScopeMismatch))
    ));
    machine.handle(&Event::Add(1), context(initial, 1)).unwrap();
    assert!(matches!(
        machine.handle(&Event::Add(1), context(initial, 2)),
        Err(MachineFailure::Kernel(MachineError::StaleHead))
    ));
    let head = machine.read().0;
    assert!(matches!(
        machine.handle(&Event::Add(1), context(head, 1)),
        Err(MachineFailure::Kernel(MachineError::DuplicateIntent))
    ));
    machine.handle(&Event::Add(1), context(head, 2)).unwrap();
    let head = machine.read().0;
    assert!(matches!(
        machine.handle(&Event::Add(1), context(head, 3)),
        Err(MachineFailure::Kernel(MachineError::Capacity))
    ));
    assert_eq!(machine.read().1.value, 2);
    assert_eq!(machine.take_intents().len(), 2);
    machine.handle(&Event::Add(1), context(head, 3)).unwrap();
    let mut zero = owner::<0>();
    assert!(matches!(
        zero.handle(&Event::Add(0), context(zero.read().0, 0)),
        Err(MachineFailure::Kernel(MachineError::Capacity))
    ));
    let mut bytes =
        MemoryMachine::<Counter, 2>::new(Counter, scope(), State { value: 0 }, limits(2, 7))
            .unwrap();
    assert!(matches!(
        bytes.handle(&Event::Add(0), context(bytes.read().0, 0)),
        Err(MachineFailure::Kernel(MachineError::Capacity))
    ));
}

#[test]
fn accepted_revision_never_wraps_and_refusal_remains_possible_at_the_limit() {
    let head = MachineHead {
        scope: scope(),
        revision: MachineRevision::new(u64::MAX),
    };
    let mut machine =
        MemoryMachine::<Counter, 2>::restore(Counter, head, State { value: 0 }, limits(2, 16))
            .unwrap();
    assert!(matches!(
        machine.handle(&Event::Add(0), context(head, 1)),
        Err(MachineFailure::Kernel(MachineError::RevisionExhausted))
    ));
    assert_eq!(
        machine.handle(&Event::Denied, context(head, 1)).unwrap(),
        HandleOutcome::Rejected {
            reason: RejectCode::new(7)
        }
    );
    assert_eq!(machine.read(), (head, &State { value: 0 }));
    assert_eq!(machine.pending_intents().count(), 0);
}

struct Bomb(Rc<Cell<bool>>);
impl Drop for Bomb {
    fn drop(&mut self) {
        if self.0.replace(false) {
            panic!("payload destructor");
        }
    }
}
struct CatchingModel {
    bomb: Rc<Cell<bool>>,
}
impl Machine for CatchingModel {
    type State = u8;
    type Event = ();
    type Effect = Bomb;
    type Error = ();
    fn definition(&self) -> MachineDefinition {
        definition()
    }
    fn validate(&self, _: &u8) -> Result<(), ()> {
        Ok(())
    }
    fn transition<S: IntentSink<Bomb>>(
        &self,
        _: &u8,
        (): &(),
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<u8>, ()> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            sink.stage(ProposedIntent {
                metadata: metadata(context, 1),
                payload: Bomb(self.bomb.clone()),
            })
        }));
        assert!(result.is_err());
        Ok(Transition::Accepted {
            next: 1,
            transition: TransitionId::new(1),
        })
    }
}
#[test]
fn a_caught_destructor_panic_cannot_unpoison_staging() {
    let mut machine = MemoryMachine::<_, 0>::new(
        CatchingModel {
            bomb: Rc::new(Cell::new(true)),
        },
        scope(),
        0,
        limits(0, 0),
    )
    .unwrap();
    let head = machine.read().0;
    assert!(matches!(
        machine.handle(&(), context(head, 1)),
        Err(MachineFailure::Kernel(MachineError::StagingInterrupted))
    ));
    assert_eq!(machine.read(), (head, &0));
    assert!(machine.last_transition().is_none());
}

struct DroppingState {
    value: u8,
    bomb: Rc<Cell<bool>>,
}
impl Drop for DroppingState {
    fn drop(&mut self) {
        if self.bomb.replace(false) {
            panic!("superseded state destructor");
        }
    }
}
struct DropModel;
impl Machine for DropModel {
    type State = DroppingState;
    type Event = ();
    type Effect = u8;
    type Error = ();
    fn definition(&self) -> MachineDefinition {
        definition()
    }
    fn validate(&self, _: &DroppingState) -> Result<(), ()> {
        Ok(())
    }
    fn transition<S: IntentSink<u8>>(
        &self,
        current: &DroppingState,
        (): &(),
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<DroppingState>, ()> {
        sink.stage(ProposedIntent {
            metadata: metadata(context, 1),
            payload: 9,
        })
        .map_err(|_admission| ())?;
        Ok(Transition::Accepted {
            next: DroppingState {
                value: 7,
                bomb: current.bomb.clone(),
            },
            transition: TransitionId::new(2),
        })
    }
}
#[test]
fn superseded_state_drop_panics_only_after_all_published_values_agree() {
    let mut machine = MemoryMachine::<_, 1>::new(
        DropModel,
        scope(),
        DroppingState {
            value: 0,
            bomb: Rc::new(Cell::new(true)),
        },
        limits(1, 1),
    )
    .unwrap();
    let head = machine.read().0;
    assert!(catch_unwind(AssertUnwindSafe(|| machine.handle(&(), context(head, 1)))).is_err());
    assert_eq!(machine.read().0.revision, MachineRevision::new(1));
    assert_eq!(machine.read().1.value, 7);
    assert_eq!(
        machine.last_transition().unwrap().transition,
        TransitionId::new(2)
    );
    let batch = machine.take_intents();
    assert_eq!(batch.len(), 1);
    assert_eq!(
        batch.iter().next().unwrap().origin,
        machine.last_transition().unwrap()
    );
}

#[test]
fn deterministic_sequences_match_a_separate_arithmetic_oracle() {
    for seed in 0..32_u64 {
        let mut machine = owner::<4>();
        let mut value = 0;
        let mut revision = 0;
        for index in 1..=80 {
            let code = (seed * 17 + index * 11) % 9;
            let event = match code {
                0 => Event::Denied,
                1 => Event::Unknown,
                2 => Event::Fault,
                _ => Event::Add(code),
            };
            let result = machine.handle(&event, context(machine.read().0, index));
            match code {
                0 => assert_eq!(
                    result.unwrap(),
                    HandleOutcome::Rejected {
                        reason: RejectCode::new(7)
                    }
                ),
                1 => assert_eq!(result.unwrap(), HandleOutcome::Unhandled),
                2 => assert!(matches!(
                    result,
                    Err(MachineFailure::Model(Fault::Evaluation))
                )),
                amount if value + amount > 100 => {
                    assert!(matches!(result, Err(MachineFailure::Model(Fault::Invalid))))
                }
                amount => {
                    assert!(matches!(result, Ok(HandleOutcome::Accepted(_))));
                    value += amount;
                    revision += 1;
                }
            }
            assert_eq!(machine.read().1.value, value);
            assert_eq!(machine.read().0.revision.get(), revision);
            let _ = machine.take_intents();
        }
    }
}
