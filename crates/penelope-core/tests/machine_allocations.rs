//! Isolated flat/hierarchy lifecycle allocation probe with a positive control.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::panic_in_result_fn
)]
#[path = "support/allocations.rs"]
mod allocations;
use allocations::{Counts, start_counting, stop_counting};
use penelope_core::machine::*;
use std::hint::black_box;

const fn definition() -> MachineDefinition {
    MachineDefinition {
        fingerprint: MachineDefinitionId::from_bytes([3; 32]),
        state_schema: 1,
        event_schema: 1,
    }
}
const fn scope() -> MachineScope {
    MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: definition(),
    }
}
const fn limits() -> IntentLimits {
    IntentLimits {
        max_intents: 2,
        max_payload_bytes: 2,
    }
}
fn context(head: MachineHead, step: u64) -> TransitionContext {
    let mut bytes = [0; 32];
    bytes
        .get_mut(..8)
        .unwrap()
        .copy_from_slice(&step.to_be_bytes());
    TransitionContext {
        expected: head,
        event: MachineEventId::from_bytes(bytes),
        event_digest: bytes,
        logical_time: MachineLogicalTime::new(step),
    }
}
fn stage<S: IntentSink<u64>>(
    context: &TransitionContext,
    sink: &mut S,
) -> Result<(), MachineError> {
    sink.stage(ProposedIntent {
        metadata: IntentMetadata {
            identity: MachineIntentId::from_bytes(context.event.bytes()),
            digest: context.event_digest,
            payload_bytes: 1,
        },
        payload: 7,
    })
}
struct Counter;
impl Machine for Counter {
    type State = u64;
    type Event = u8;
    type Effect = u64;
    type Error = MachineError;
    fn definition(&self) -> MachineDefinition {
        definition()
    }
    fn validate(&self, state: &u64) -> Result<(), MachineError> {
        if *state > 100_000 {
            Err(MachineError::Capacity)
        } else {
            Ok(())
        }
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &u64,
        event: &u8,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<u64>, MachineError> {
        stage(context, sink)?;
        match event {
            0 => Ok(Transition::Unhandled),
            1 => Ok(Transition::Rejected {
                reason: RejectCode::new(1),
            }),
            2 => Err(MachineError::Capacity),
            3 => Ok(Transition::Accepted {
                next: 100_001,
                transition: TransitionId::new(1),
            }),
            _ => Ok(Transition::Accepted {
                next: current.checked_add(1).unwrap(),
                transition: TransitionId::new(1),
            }),
        }
    }
}
struct Tree;
impl HierarchyModel for Tree {
    type State = u8;
    type Event = ();
    type Effect = u64;
    type Error = MachineError;
    type Node = u8;
    fn definition(&self) -> MachineDefinition {
        definition()
    }
    fn leaf(&self, state: &u8) -> u8 {
        *state
    }
    fn parent(&self, node: u8) -> Option<u8> {
        if node == 0 { None } else { Some(0) }
    }
    fn validate(&self, _: &u8) -> Result<(), MachineError> {
        Ok(())
    }
    fn select(
        &self,
        node: u8,
        current: &u8,
        (): &(),
        _: &TransitionContext,
    ) -> Result<HierarchyDecision<u8>, MachineError> {
        if node == 0 {
            Ok(HierarchyDecision::Selected(HierarchyPlan {
                target: if *current == 1 { 2 } else { 1 },
                kind: TransitionKind::External,
                transition: TransitionId::new(1),
            }))
        } else {
            Ok(HierarchyDecision::Unhandled)
        }
    }
    fn candidate(
        &self,
        plan: HierarchyPlan<u8>,
        _: &u8,
        (): &(),
        _: &TransitionContext,
    ) -> Result<u8, MachineError> {
        Ok(plan.target)
    }
    fn action<S: IntentSink<u64>>(
        &self,
        _: HierarchyPlan<u8>,
        _: &u8,
        _: &mut u8,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), MachineError> {
        stage(context, sink)
    }
}
fn exercise() {
    let mut flat = MemoryMachine::<_, 2>::new(Counter, scope(), 0, limits()).unwrap();
    let mut nested =
        MemoryMachine::<_, 2>::new(Hierarchical::<_, 2>::new(Tree), scope(), 1, limits()).unwrap();
    for step in 0..10_000 {
        let event = u8::try_from(step % 8).unwrap();
        let context = context(flat.read().0, step);
        if step % 2 == 0 {
            let _outcome = black_box(flat.handle(&event, context));
        } else {
            let _candidate = black_box(flat.prepare(&event, context));
        } // Dropping a ready handle aborts.
        let _ = black_box(flat.take_intents());
        let nested_context = self::context(nested.read().0, step);
        black_box(nested.handle(&(), nested_context).unwrap());
        let _ = black_box(nested.take_intents());
    }
}
fn main() {
    start_counting();
    let control = black_box(Vec::<u8>::with_capacity(16));
    drop(black_box(control));
    assert_eq!(
        stop_counting(),
        Counts {
            allocations: 1,
            reallocations: 0,
            deallocations: 1,
            requested_bytes: 16
        }
    );
    start_counting();
    black_box(exercise)();
    let counts = stop_counting();
    assert_eq!(
        counts,
        Counts {
            allocations: 0,
            reallocations: 0,
            deallocations: 0,
            requested_bytes: 0
        }
    );
    println!("flat and hierarchical machine lifecycles: 10000 dispatches each, {counts:?}");
}
