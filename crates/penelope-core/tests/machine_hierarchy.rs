//! Hierarchy order, bounded paths, terminal guards and failure isolation.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::panic_in_result_fn,
    clippy::shadow_unrelated
)]
use penelope_core::machine::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const PARENTS: [Option<u8>; 7] = [None, Some(0), Some(0), Some(1), Some(1), Some(2), Some(2)];
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Trace {
    Exit(u8),
    Action,
    Enter(u8),
}
struct State {
    leaf: u8,
    value: u8,
}
#[derive(Debug, PartialEq, Eq)]
enum Fault {
    Guard,
    Hook,
    Invalid,
}
enum Event {
    Go {
        target: u8,
        handler: u8,
        kind: TransitionKind,
    },
    Reject(u8),
    Fault(u8),
    Unknown,
}
struct Tree {
    parents: [Option<u8>; 7],
    fail: Option<Trace>,
    ignore_stage_error: bool,
    bad_candidate: bool,
    bad_invariant: bool,
    mutate_target: bool,
    visited: Rc<RefCell<Vec<u8>>>,
    hooks: Rc<Cell<u32>>,
}
impl Tree {
    fn normal() -> Self {
        Self {
            parents: PARENTS,
            fail: None,
            ignore_stage_error: false,
            bad_candidate: false,
            bad_invariant: false,
            mutate_target: false,
            visited: Rc::default(),
            hooks: Rc::default(),
        }
    }
    fn hook<S: IntentSink<Trace>>(
        &self,
        trace: Trace,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), Fault> {
        self.hooks.set(self.hooks.get() + 1);
        let mut bytes = context.event.bytes();
        let code = match trace {
            Trace::Exit(node) => node,
            Trace::Action => 99,
            Trace::Enter(node) => node + 16,
        };
        *bytes.last_mut().unwrap() = code;
        let staged = sink.stage(ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(bytes),
                digest: [code; 32],
                payload_bytes: 1,
            },
            payload: trace,
        });
        if !self.ignore_stage_error {
            staged.map_err(|_admission| Fault::Hook)?;
        }
        if self.fail == Some(trace) {
            Err(Fault::Hook)
        } else {
            Ok(())
        }
    }
}
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
const fn limits(count: usize) -> IntentLimits {
    IntentLimits {
        max_intents: count,
        max_payload_bytes: 100,
    }
}
const fn context(head: MachineHead) -> TransitionContext {
    TransitionContext {
        expected: head,
        event: MachineEventId::from_bytes([8; 32]),
        event_digest: [9; 32],
        logical_time: MachineLogicalTime::ZERO,
    }
}
impl HierarchyModel for Tree {
    type State = State;
    type Event = Event;
    type Effect = Trace;
    type Error = Fault;
    type Node = u8;
    fn definition(&self) -> MachineDefinition {
        definition()
    }
    fn leaf(&self, state: &State) -> u8 {
        state.leaf
    }
    fn parent(&self, node: u8) -> Option<u8> {
        self.parents.get(usize::from(node)).copied().flatten()
    }
    fn validate(&self, state: &State) -> Result<(), Fault> {
        if state.value > 10 {
            Err(Fault::Invalid)
        } else {
            Ok(())
        }
    }
    fn select(
        &self,
        node: u8,
        current: &State,
        event: &Event,
        _: &TransitionContext,
    ) -> Result<HierarchyDecision<u8>, Fault> {
        self.visited.borrow_mut().push(node);
        match event {
            Event::Go {
                target,
                handler,
                kind,
            } if node == *handler => Ok(HierarchyDecision::Selected(HierarchyPlan {
                target: *target,
                kind: *kind,
                transition: TransitionId::new(9),
            })),
            Event::Reject(at) if node == *at => Ok(HierarchyDecision::Rejected {
                reason: RejectCode::new(4),
            }),
            Event::Fault(at) if node == *at => Err(Fault::Guard),
            Event::Reject(_) | Event::Fault(_) if node == 0 => {
                Ok(HierarchyDecision::Selected(HierarchyPlan {
                    target: current.leaf,
                    kind: TransitionKind::Internal,
                    transition: TransitionId::new(1),
                }))
            }
            Event::Go { .. } | Event::Reject(_) | Event::Fault(_) | Event::Unknown => {
                Ok(HierarchyDecision::Unhandled)
            }
        }
    }
    fn candidate(
        &self,
        plan: HierarchyPlan<u8>,
        current: &State,
        _: &Event,
        _: &TransitionContext,
    ) -> Result<State, Fault> {
        Ok(State {
            leaf: if self.bad_candidate { 0 } else { plan.target },
            value: current.value,
        })
    }
    fn exit<S: IntentSink<Trace>>(
        &self,
        node: u8,
        _: &State,
        _: &mut State,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), Fault> {
        self.hook(Trace::Exit(node), context, sink)
    }
    fn action<S: IntentSink<Trace>>(
        &self,
        _: HierarchyPlan<u8>,
        _: &State,
        next: &mut State,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), Fault> {
        next.value = if self.bad_invariant { 11 } else { 1 };
        if self.mutate_target {
            next.leaf = 0;
        }
        self.hook(Trace::Action, context, sink)
    }
    fn enter<S: IntentSink<Trace>>(
        &self,
        node: u8,
        _: &State,
        _: &mut State,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), Fault> {
        self.hook(Trace::Enter(node), context, sink)
    }
}
const fn go(target: u8, handler: u8, kind: TransitionKind) -> Event {
    Event::Go {
        target,
        handler,
        kind,
    }
}

#[test]
fn leaf_to_parent_selection_and_lca_exit_action_entry_order_are_exact() {
    let tree = Tree::normal();
    let visited = tree.visited.clone();
    let mut owner = MemoryMachine::<_, 8>::new(
        Hierarchical::<_, 3>::new(tree),
        scope(),
        State { leaf: 3, value: 0 },
        limits(8),
    )
    .unwrap();
    owner
        .handle(&go(6, 0, TransitionKind::External), context(owner.read().0))
        .unwrap();
    assert_eq!(*visited.borrow(), vec![3, 1, 0]);
    assert_eq!(owner.read().1.leaf, 6);
    assert_eq!(owner.read().1.value, 1);
    assert_eq!(
        owner
            .take_intents()
            .into_iter()
            .map(|entry| entry.intent.payload)
            .collect::<Vec<_>>(),
        vec![
            Trace::Exit(3),
            Trace::Exit(1),
            Trace::Action,
            Trace::Enter(2),
            Trace::Enter(6)
        ]
    );
}

#[test]
fn rejection_and_fault_are_terminal_but_unhandled_walks_the_entire_path() {
    for event in [Event::Reject(1), Event::Fault(1), Event::Unknown] {
        let tree = Tree::normal();
        let visited = tree.visited.clone();
        let mut owner = MemoryMachine::<_, 8>::new(
            Hierarchical::<_, 3>::new(tree),
            scope(),
            State { leaf: 3, value: 0 },
            limits(8),
        )
        .unwrap();
        let head = owner.read().0;
        let result = owner.handle(&event, context(head));
        match event {
            Event::Reject(_) => {
                assert_eq!(
                    result.unwrap(),
                    HandleOutcome::Rejected {
                        reason: RejectCode::new(4)
                    }
                );
                assert_eq!(*visited.borrow(), vec![3, 1]);
            }
            Event::Fault(_) => {
                assert!(matches!(
                    result,
                    Err(MachineFailure::Model(HierarchyFailure::Model(Fault::Guard)))
                ));
                assert_eq!(*visited.borrow(), vec![3, 1]);
            }
            Event::Unknown => {
                assert_eq!(result.unwrap(), HandleOutcome::Unhandled);
                assert_eq!(*visited.borrow(), vec![3, 1, 0]);
            }
            Event::Go { .. } => panic!("unexpected event"),
        }
        assert_eq!(owner.read().0, head);
        assert_eq!(owner.read().1.leaf, 3);
        assert_eq!(owner.pending_intents().count(), 0);
    }
}

#[test]
fn internal_and_external_self_transitions_are_distinct() {
    for (kind, expected) in [
        (TransitionKind::Internal, vec![Trace::Action]),
        (
            TransitionKind::External,
            vec![Trace::Exit(3), Trace::Action, Trace::Enter(3)],
        ),
    ] {
        let mut owner = MemoryMachine::<_, 8>::new(
            Hierarchical::<_, 3>::new(Tree::normal()),
            scope(),
            State { leaf: 3, value: 0 },
            limits(8),
        )
        .unwrap();
        owner
            .handle(&go(3, 1, kind), context(owner.read().0))
            .unwrap();
        assert_eq!(
            owner
                .take_intents()
                .into_iter()
                .map(|entry| entry.intent.payload)
                .collect::<Vec<_>>(),
            expected
        );
    }
    let mut owner = MemoryMachine::<_, 8>::new(
        Hierarchical::<_, 3>::new(Tree::normal()),
        scope(),
        State { leaf: 3, value: 0 },
        limits(8),
    )
    .unwrap();
    assert!(matches!(
        owner.handle(&go(4, 1, TransitionKind::Internal), context(owner.read().0)),
        Err(MachineFailure::Model(HierarchyFailure::Kernel(
            MachineError::InvalidHierarchy
        )))
    ));
}

#[test]
fn malformed_or_exhausted_paths_fail_before_any_handler() {
    let tree = Tree::normal();
    let visited = tree.visited.clone();
    assert!(matches!(
        MemoryMachine::<_, 8>::new(
            Hierarchical::<_, 2>::new(tree),
            scope(),
            State { leaf: 3, value: 0 },
            limits(8)
        ),
        Err(MachineFailure::Model(HierarchyFailure::Kernel(
            MachineError::HierarchyDepth
        )))
    ));
    assert!(visited.borrow().is_empty());
    let mut tree = Tree::normal();
    *tree.parents.get_mut(1).unwrap() = Some(3);
    assert!(matches!(
        MemoryMachine::<_, 8>::new(
            Hierarchical::<_, 7>::new(tree),
            scope(),
            State { leaf: 3, value: 0 },
            limits(8)
        ),
        Err(MachineFailure::Model(HierarchyFailure::Kernel(
            MachineError::InvalidHierarchy
        )))
    ));
    assert!(matches!(
        MemoryMachine::<_, 8>::new(
            Hierarchical::<_, 0>::new(Tree::normal()),
            scope(),
            State { leaf: 0, value: 0 },
            limits(8)
        ),
        Err(MachineFailure::Model(HierarchyFailure::Kernel(
            MachineError::HierarchyDepth
        )))
    ));
    let mut tree = Tree::normal();
    *tree.parents.get_mut(6).unwrap() = Some(6);
    let mut owner = MemoryMachine::<_, 8>::new(
        Hierarchical::<_, 3>::new(tree),
        scope(),
        State { leaf: 3, value: 0 },
        limits(8),
    )
    .unwrap();
    assert!(matches!(
        owner.handle(&go(6, 0, TransitionKind::External), context(owner.read().0)),
        Err(MachineFailure::Model(HierarchyFailure::Kernel(
            MachineError::InvalidHierarchy
        )))
    ));
    assert_eq!(owner.read().1.leaf, 3);
    assert_eq!(owner.pending_intents().count(), 0);
}

#[test]
fn each_hook_failure_bad_candidate_and_invariant_failure_discards_the_whole_attempt() {
    for fail in [
        Trace::Exit(3),
        Trace::Exit(1),
        Trace::Action,
        Trace::Enter(2),
        Trace::Enter(6),
    ] {
        let mut tree = Tree::normal();
        tree.fail = Some(fail);
        let mut owner = MemoryMachine::<_, 8>::new(
            Hierarchical::<_, 3>::new(tree),
            scope(),
            State { leaf: 3, value: 0 },
            limits(8),
        )
        .unwrap();
        let head = owner.read().0;
        assert!(matches!(
            owner.handle(&go(6, 0, TransitionKind::External), context(head)),
            Err(MachineFailure::Model(HierarchyFailure::Model(Fault::Hook)))
        ));
        assert_eq!(owner.read().0, head);
        assert_eq!(owner.read().1.leaf, 3);
        assert_eq!(owner.read().1.value, 0);
        assert_eq!(owner.pending_intents().count(), 0);
    }
    for kind in 0..3 {
        let mut tree = Tree::normal();
        tree.bad_candidate = kind == 0;
        tree.bad_invariant = kind == 1;
        tree.mutate_target = kind == 2;
        let mut owner = MemoryMachine::<_, 8>::new(
            Hierarchical::<_, 3>::new(tree),
            scope(),
            State { leaf: 3, value: 0 },
            limits(8),
        )
        .unwrap();
        let head = owner.read().0;
        assert!(
            owner
                .handle(&go(6, 0, TransitionKind::External), context(head))
                .is_err()
        );
        assert_eq!(owner.read().0, head);
        assert_eq!(owner.read().1.leaf, 3);
        assert_eq!(owner.pending_intents().count(), 0);
    }
}

#[test]
fn ignored_hook_staging_failure_stops_the_remaining_hooks() {
    let mut tree = Tree::normal();
    tree.ignore_stage_error = true;
    let hooks = tree.hooks.clone();
    let mut owner = MemoryMachine::<_, 1>::new(
        Hierarchical::<_, 3>::new(tree),
        scope(),
        State { leaf: 3, value: 0 },
        limits(1),
    )
    .unwrap();
    let head = owner.read().0;
    assert!(matches!(
        owner.handle(&go(6, 0, TransitionKind::External), context(head)),
        Err(MachineFailure::Kernel(MachineError::Capacity))
    ));
    assert_eq!(hooks.get(), 2); // First exit admitted; second failed; no action or entries.
    assert_eq!(owner.read().0, head);
    assert_eq!(owner.pending_intents().count(), 0);
}

#[test]
fn every_pair_of_tree_states_matches_an_independent_path_oracle() {
    fn path(mut node: u8) -> Vec<u8> {
        let mut result = vec![node];
        while let Some(parent) = PARENTS.get(usize::from(node)).copied().flatten() {
            result.push(parent);
            node = parent;
        }
        result
    }
    for source in 0..7 {
        for target in 0..7 {
            let mut before = path(source);
            let mut after = path(target);
            while before.last().is_some() && before.last() == after.last() {
                before.pop();
                after.pop();
            }
            if source == target {
                before.push(source);
                after.push(target);
            }
            let expected = before
                .into_iter()
                .map(Trace::Exit)
                .chain([Trace::Action])
                .chain(after.into_iter().rev().map(Trace::Enter))
                .collect::<Vec<_>>();
            let mut owner = MemoryMachine::<_, 8>::new(
                Hierarchical::<_, 3>::new(Tree::normal()),
                scope(),
                State {
                    leaf: source,
                    value: 0,
                },
                limits(8),
            )
            .unwrap();
            owner
                .handle(
                    &go(target, 0, TransitionKind::External),
                    context(owner.read().0),
                )
                .unwrap();
            assert_eq!(owner.read().1.leaf, target);
            assert_eq!(
                owner
                    .take_intents()
                    .into_iter()
                    .map(|entry| entry.intent.payload)
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
