//! Independent bounded graph/path oracle shared by deterministic tests and fuzzing.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use penelope_core::machine::*;

const NODES: usize = 8;
const EVENTS: usize = 4;
const CAPACITY: usize = 17; // Eight exits, one action, eight entries.
#[derive(Clone, Copy)]
struct Table {
    parents: [Option<u8>; NODES],
    rules: [[u8; EVENTS]; NODES],
}
const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([31; 32]),
    state_schema: 1,
    event_schema: 1,
};
const fn scope() -> MachineScope {
    MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: DEFINITION,
    }
}
fn stage<S: IntentSink<u8>>(code: u8, sink: &mut S) -> Result<(), u8> {
    sink.stage(ProposedIntent {
        metadata: IntentMetadata {
            identity: MachineIntentId::from_bytes([code; 32]),
            digest: [code; 32],
            payload_bytes: 1,
        },
        payload: code,
    })
    .map_err(|_admission| 255)
}
impl HierarchyModel for Table {
    type State = u8;
    type Event = u8;
    type Effect = u8;
    type Error = u8;
    type Node = u8;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn leaf(&self, state: &u8) -> u8 {
        *state
    }
    fn parent(&self, node: u8) -> Option<u8> {
        self.parents[usize::from(node)]
    }
    fn validate(&self, state: &u8) -> Result<(), u8> {
        if *state < 8 { Ok(()) } else { Err(254) }
    }
    fn select(
        &self,
        node: u8,
        current: &u8,
        event: &u8,
        _: &TransitionContext,
    ) -> Result<HierarchyDecision<u8>, u8> {
        let rule = self.rules[usize::from(node)][usize::from(*event)];
        Ok(match rule % 5 {
            0 => HierarchyDecision::Unhandled,
            1 => HierarchyDecision::Rejected {
                reason: RejectCode::new(7),
            },
            2 => return Err(55),
            kind => HierarchyDecision::Selected(HierarchyPlan {
                target: if kind == 3 { *current } else { (rule / 5) % 8 },
                kind: if kind == 3 {
                    TransitionKind::Internal
                } else {
                    TransitionKind::External
                },
                transition: TransitionId::new(u64::from(node) * 4 + u64::from(*event) + 1),
            }),
        })
    }
    fn candidate(
        &self,
        plan: HierarchyPlan<u8>,
        _: &u8,
        _: &u8,
        _: &TransitionContext,
    ) -> Result<u8, u8> {
        Ok(plan.target)
    }
    fn exit<S: IntentSink<u8>>(
        &self,
        node: u8,
        _: &u8,
        _: &mut u8,
        _: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), u8> {
        stage(node, sink)
    }
    fn action<S: IntentSink<u8>>(
        &self,
        _: HierarchyPlan<u8>,
        _: &u8,
        _: &mut u8,
        _: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), u8> {
        stage(8, sink)
    }
    fn enter<S: IntentSink<u8>>(
        &self,
        node: u8,
        _: &u8,
        _: &mut u8,
        _: &TransitionContext,
        sink: &mut S,
    ) -> Result<(), u8> {
        stage(node + 9, sink)
    }
}
// A direct flat dispatcher, not an invocation of Hierarchical or its path logic.
impl Machine for Table {
    type State = u8;
    type Event = u8;
    type Effect = u8;
    type Error = u8;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, state: &u8) -> Result<(), u8> {
        HierarchyModel::validate(self, state)
    }
    fn transition<S: IntentSink<u8>>(
        &self,
        current: &u8,
        event: &u8,
        _: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<u8>, u8> {
        let rule = self.rules[usize::from(*current)][usize::from(*event)];
        match rule % 5 {
            0 => Ok(Transition::Unhandled),
            1 => Ok(Transition::Rejected {
                reason: RejectCode::new(7),
            }),
            2 => Err(55),
            kind => {
                let next = if kind == 3 { *current } else { (rule / 5) % 8 };
                if kind == 4 {
                    stage(*current, sink)?;
                }
                stage(8, sink)?;
                if kind == 4 {
                    stage(next + 9, sink)?;
                }
                Ok(Transition::Accepted {
                    next,
                    transition: TransitionId::new(u64::from(*current) * 4 + u64::from(*event) + 1),
                })
            }
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
enum Expected {
    Accepted {
        state: u8,
        transition: TransitionId,
        trace: Vec<u8>,
    },
    Rejected,
    Unhandled,
    Fault,
    Capacity,
}
// Reference uses ordinary ancestor lists and set membership, independently of
// the production fixed-slot StatePath and reversed common-prefix calculation.
fn ancestors(table: Table, mut node: u8) -> Vec<u8> {
    let mut result = vec![node];
    while let Some(parent) = table.parents[usize::from(node)] {
        assert!(!result.contains(&parent));
        result.push(parent);
        node = parent;
    }
    result
}
fn reference(table: Table, state: u8, event: u8, budget: usize) -> Expected {
    let source = ancestors(table, state);
    let Some((handler, rule)) = source.iter().find_map(|node| {
        let rule = table.rules[usize::from(*node)][usize::from(event)];
        (!rule.is_multiple_of(5)).then_some((*node, rule))
    }) else {
        return Expected::Unhandled;
    };
    match rule % 5 {
        1 => return Expected::Rejected,
        2 => return Expected::Fault,
        _ => {}
    }
    let next = if rule % 5 == 3 { state } else { (rule / 5) % 8 };
    let mut trace = Vec::new();
    if rule % 5 == 4 {
        let target = ancestors(table, next);
        let common = source.iter().copied().find(|node| target.contains(node));
        if next == state {
            trace.push(state);
        } else {
            trace.extend(
                source
                    .iter()
                    .copied()
                    .take_while(|node| Some(*node) != common),
            );
        }
        trace.push(8);
        if next == state {
            trace.push(next + 9);
        } else {
            let mut entries: Vec<_> = target
                .iter()
                .copied()
                .take_while(|node| Some(*node) != common)
                .collect();
            entries.reverse();
            trace.extend(entries.into_iter().map(|node| node + 9));
        }
    } else {
        trace.push(8);
    }
    if trace.len() > budget {
        return Expected::Capacity;
    }
    Expected::Accepted {
        state: next,
        transition: TransitionId::new(u64::from(handler) * 4 + u64::from(event) + 1),
        trace,
    }
}
#[allow(clippy::needless_pass_by_value)] // Match Result::map_err ownership.
const fn flatten(error: MachineFailure<HierarchyFailure<u8>>) -> MachineFailure<u8> {
    match error {
        MachineFailure::Kernel(error) | MachineFailure::Model(HierarchyFailure::Kernel(error)) => {
            MachineFailure::Kernel(error)
        }
        MachineFailure::Model(HierarchyFailure::Model(error)) => MachineFailure::Model(error),
    }
}
fn exercise<M: Machine<State = u8, Event = u8, Effect = u8>>(
    model: M,
    table: Table,
    initial: u8,
    budget: usize,
    events: &[u8],
    normalize: impl Fn(MachineFailure<M::Error>) -> MachineFailure<u8>,
) -> Vec<TransitionRecord>
where
    M::Error: core::fmt::Debug,
{
    let mut owner = MemoryMachine::<_, CAPACITY>::new(
        model,
        scope(),
        initial,
        IntentLimits {
            max_intents: budget,
            max_payload_bytes: budget as u64,
        },
    )
    .unwrap();
    let mut state = initial;
    let mut revision = 0;
    let mut records = Vec::new();
    for (index, byte) in events.iter().take(96).enumerate() {
        let event = byte % 4;
        let mut identity = [0; 32];
        identity[..8].copy_from_slice(&(index as u64 + 1).to_be_bytes());
        let context = TransitionContext {
            expected: MachineHead {
                scope: scope(),
                revision: MachineRevision::new(revision),
            },
            event: MachineEventId::from_bytes(identity),
            event_digest: identity,
            logical_time: MachineLogicalTime::new(index as u64),
        };
        let before = owner.last_transition();
        let expected = reference(table, state, event, budget);
        let actual = owner.handle(&event, context).map_err(&normalize);
        match expected {
            Expected::Accepted {
                state: next,
                transition,
                trace,
            } => {
                revision += 1;
                let expected_record = TransitionRecord {
                    context,
                    revision: MachineRevision::new(revision),
                    transition,
                };
                assert_eq!(actual.unwrap(), HandleOutcome::Accepted(expected_record));
                state = next;
                assert_eq!(
                    owner
                        .pending_intents()
                        .map(|entry| entry.intent.payload)
                        .collect::<Vec<_>>(),
                    trace
                );
                assert!(
                    owner
                        .pending_intents()
                        .all(|entry| entry.origin == expected_record)
                );
                assert_eq!(owner.last_transition(), Some(expected_record));
                records.push(expected_record);
            }
            Expected::Rejected => assert_eq!(
                actual.unwrap(),
                HandleOutcome::Rejected {
                    reason: RejectCode::new(7)
                }
            ),
            Expected::Unhandled => assert_eq!(actual.unwrap(), HandleOutcome::Unhandled),
            Expected::Fault => assert!(matches!(actual, Err(MachineFailure::Model(55)))),
            Expected::Capacity => assert!(matches!(
                actual,
                Err(MachineFailure::Kernel(MachineError::Capacity))
            )),
        }
        assert_eq!(
            owner.read(),
            (
                MachineHead {
                    scope: scope(),
                    revision: MachineRevision::new(revision)
                },
                &state
            )
        );
        if revision == before.map_or(0, |record| record.revision.get()) {
            assert_eq!(owner.last_transition(), before);
            assert_eq!(owner.pending_intents().count(), 0);
        }
        let _ = owner.take_intents();
    }
    records
}
/// Data varies the transition table, forest shape, initial leaf, budget and events.
/// A second replay and a single-level specialization check exact provenance too.
pub fn check(data: &[u8]) {
    let byte = |index| data.get(index).copied().unwrap_or(0);
    let table = Table {
        parents: core::array::from_fn(|node| {
            let parent = usize::from(byte(node)) % (node + 1);
            (parent != node).then_some(parent as u8)
        }),
        rules: core::array::from_fn(|node| {
            core::array::from_fn(|event| byte(8 + node * EVENTS + event))
        }),
    };
    let initial = byte(40) % 8;
    let budget = usize::from(byte(41)) % (CAPACITY + 1);
    let fallback = [0, 1, 2, 3];
    let events = data
        .get(42..)
        .filter(|events| !events.is_empty())
        .unwrap_or(&fallback);
    let records = exercise(
        Hierarchical::<_, 8>::new(table),
        table,
        initial,
        budget,
        events,
        flatten,
    );
    assert_eq!(
        exercise(
            Hierarchical::<_, 8>::new(table),
            table,
            initial,
            budget,
            events,
            flatten
        ),
        records
    );
    let flat = Table {
        parents: [None; NODES],
        ..table
    };
    let direct = exercise(flat, flat, initial, budget, events, core::convert::identity);
    assert_eq!(
        exercise(
            Hierarchical::<_, 1>::new(flat),
            flat,
            initial,
            budget,
            events,
            flatten
        ),
        direct
    );
    // Malformed restored paths must refuse before dispatch, without a loop.
    let mut cyclic = table;
    cyclic.parents[usize::from(initial)] = Some(initial);
    let result = MemoryMachine::<_, CAPACITY>::new(
        Hierarchical::<_, 8>::new(cyclic),
        scope(),
        initial,
        IntentLimits {
            max_intents: budget,
            max_payload_bytes: budget as u64,
        },
    );
    assert!(matches!(
        result,
        Err(MachineFailure::Model(HierarchyFailure::Kernel(
            MachineError::InvalidHierarchy
        )))
    ));
    let shallow = MemoryMachine::<_, CAPACITY>::new(
        Hierarchical::<_, 1>::new(table),
        scope(),
        initial,
        IntentLimits {
            max_intents: budget,
            max_payload_bytes: budget as u64,
        },
    );
    assert_eq!(
        shallow.is_ok(),
        table.parents[usize::from(initial)].is_none()
    );
}
