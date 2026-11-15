//! Hosted active-path dispatch probe; hooks update isolated counters, never perform I/O.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
#[path = "support/measurement.rs"]
mod measurement;
use measurement::{BATCH, SAMPLES};
use penelope_core::machine::*;
use std::hint::black_box;

#[derive(Debug, PartialEq, Eq)]
struct State {
    leaf: u8,
    exits: u64,
    entries: u64,
    actions: u64,
}
#[derive(Clone, Copy)]
enum Event {
    Internal,
    External,
    Reject,
    Unhandled,
}
struct Tree {
    depth: u8,
    handler: u8,
}
const fn definition() -> MachineDefinition {
    MachineDefinition {
        fingerprint: MachineDefinitionId::from_bytes([4; 32]),
        state_schema: 1,
        event_schema: 1,
    }
}
impl HierarchyModel for Tree {
    type State = State;
    type Event = Event;
    type Effect = ();
    type Error = MachineError;
    type Node = u8;
    fn definition(&self) -> MachineDefinition {
        definition()
    }
    fn leaf(&self, state: &State) -> u8 {
        state.leaf
    }
    fn parent(&self, node: u8) -> Option<u8> {
        match node {
            0 => None,
            17 => Some(0),
            _ => Some(node - 1),
        }
    }
    fn validate(&self, state: &State) -> Result<(), MachineError> {
        if state.leaf % 16 == self.depth - 1 {
            Ok(())
        } else {
            Err(MachineError::InvalidHierarchy)
        }
    }
    fn select(
        &self,
        node: u8,
        current: &State,
        event: &Event,
        _: &TransitionContext,
    ) -> Result<HierarchyDecision<u8>, MachineError> {
        if matches!(event, Event::Unhandled) || node % 16 != self.handler {
            return Ok(HierarchyDecision::Unhandled);
        }
        if matches!(event, Event::Reject) {
            return Ok(HierarchyDecision::Rejected {
                reason: RejectCode::new(1),
            });
        }
        let target = if matches!(event, Event::Internal) || self.depth == 1 {
            current.leaf
        } else {
            current.leaf ^ 16
        };
        Ok(HierarchyDecision::Selected(HierarchyPlan {
            target,
            kind: if matches!(event, Event::Internal) {
                TransitionKind::Internal
            } else {
                TransitionKind::External
            },
            transition: TransitionId::new(1),
        }))
    }
    fn candidate(
        &self,
        plan: HierarchyPlan<u8>,
        current: &State,
        _: &Event,
        _: &TransitionContext,
    ) -> Result<State, MachineError> {
        Ok(State {
            leaf: plan.target,
            exits: current.exits,
            entries: current.entries,
            actions: current.actions,
        })
    }
    fn exit<S: IntentSink<()>>(
        &self,
        node: u8,
        _: &State,
        next: &mut State,
        _: &TransitionContext,
        _: &mut S,
    ) -> Result<(), MachineError> {
        next.exits += u64::from(node) + 1;
        Ok(())
    }
    fn action<S: IntentSink<()>>(
        &self,
        _: HierarchyPlan<u8>,
        _: &State,
        next: &mut State,
        _: &TransitionContext,
        _: &mut S,
    ) -> Result<(), MachineError> {
        next.actions += 1;
        Ok(())
    }
    fn enter<S: IntentSink<()>>(
        &self,
        node: u8,
        _: &State,
        next: &mut State,
        _: &TransitionContext,
        _: &mut S,
    ) -> Result<(), MachineError> {
        next.entries += u64::from(node) + 1;
        Ok(())
    }
}
fn profile<const DEPTH: usize>(handler: u8, label: &str, event: Event, mode: &str) {
    let depth = u8::try_from(DEPTH).unwrap();
    let scope = MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: definition(),
    };
    let mut owner = MemoryMachine::<_, 0>::new(
        Hierarchical::<_, DEPTH>::new(Tree { depth, handler }),
        scope,
        State {
            leaf: depth - 1,
            exits: 0,
            entries: 0,
            actions: 0,
        },
        IntentLimits {
            max_intents: 0,
            max_payload_bytes: 0,
        },
    )
    .unwrap();
    let mut operation = |step: u64| {
        let mut bytes = [0; 32];
        bytes[..8].copy_from_slice(&step.to_be_bytes());
        let outcome = owner
            .handle(
                black_box(&event),
                TransitionContext {
                    expected: owner.read().0,
                    event: MachineEventId::from_bytes(bytes),
                    event_digest: bytes,
                    logical_time: MachineLogicalTime::new(step),
                },
            )
            .unwrap();
        match event {
            Event::Internal | Event::External => {
                assert!(matches!(outcome, HandleOutcome::Accepted(_)))
            }
            Event::Reject => assert_eq!(
                outcome,
                HandleOutcome::Rejected {
                    reason: RejectCode::new(1)
                }
            ),
            Event::Unhandled => assert_eq!(outcome, HandleOutcome::Unhandled),
        }
        black_box(owner.read().1);
        assert!(owner.take_intents().is_empty());
    };
    operation(0);
    let measurements = measurement::measure(operation);
    let operations = (1 + 128 + SAMPLES * BATCH + BATCH) as u64;
    let accepted = matches!(event, Event::Internal | Event::External);
    let external = matches!(event, Event::External);
    let mut expected = State {
        leaf: depth - 1,
        exits: 0,
        entries: 0,
        actions: if accepted { operations } else { 0 },
    };
    if external {
        if depth == 1 {
            expected.exits = operations;
            expected.entries = operations;
        } else {
            // Independent closed-form sums for alternate sibling paths, excluding root.
            let d = u64::from(depth);
            let left = (d - 1) * (d + 2) / 2;
            let right = left + 16 * (d - 1);
            expected.exits = (operations / 2) * (left + right) + (operations % 2) * left;
            expected.entries = (operations / 2) * (left + right) + (operations % 2) * right;
            if operations % 2 == 1 {
                expected.leaf ^= 16;
            }
        }
    }
    assert_eq!(owner.read().1, &expected);
    assert_eq!(
        owner.read().0.revision,
        MachineRevision::new(if accepted { operations } else { 0 })
    );
    measurements.report(&format!(
        "hierarchy,{DEPTH},{label},{mode},{},{}",
        std::mem::size_of::<State>(),
        std::mem::size_of_val(&owner)
    ));
}
fn matrix<const DEPTH: usize>() {
    let depth = u8::try_from(DEPTH).unwrap();
    for (handler, label) in [
        (depth - 1, "leaf"),
        (depth.saturating_sub(2), "parent"),
        (0, "root"),
    ] {
        for (event, mode) in [
            (Event::Internal, "internal"),
            (Event::External, "external_sibling"),
            (Event::Reject, "reject"),
            (Event::Unhandled, "unhandled"),
        ] {
            profile::<DEPTH>(handler, label, event, mode);
        }
    }
}
fn main() {
    measurement::positive_control();
    println!(
        "kind,depth,handler,mode,state_bytes,owner_bytes,batch_mean_p50_ns,batch_mean_p95_ns,batch_mean_p99_ns,allocations_per_op,total_elapsed_ns,samples,batch,operations_per_second,raw_batch_elapsed_ns"
    );
    matrix::<1>();
    matrix::<2>();
    matrix::<4>();
    matrix::<8>();
    matrix::<16>();
}
