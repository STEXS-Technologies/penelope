//! One active nested state path, using the same MemoryMachine publication API.
use penelope::machine::*;

// No Clone or serialization is required. State-local receipt data exists only in Paid.
#[derive(Debug)]
enum Open {
    Draft,
    AwaitingPayment,
}
#[derive(Debug)]
enum OrderState {
    Open(Open),
    Paid { receipt: u64 },
    Cancelled,
}
#[derive(Debug)]
enum OrderEvent {
    Submit,
    PaymentConfirmed(u64),
    RefreshReceipt,
    Cancel,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Node {
    Root,
    Open,
    Draft,
    AwaitingPayment,
    Paid,
    Cancelled,
}
#[derive(Debug)]
enum OrderEffect {
    Exit(Node),
    Enter(Node),
    RequestPayment,
    NotifyCancellation,
}
impl OrderEffect {
    // Trace hooks stage bounded data; logging happens only after publication.
    fn stage<S: IntentSink<Self>>(
        self,
        context: &TransitionContext,
        intents: &mut S,
    ) -> Result<(), MachineError> {
        let code = match self {
            Self::Exit(node) => node as u8,
            Self::RequestPayment => 6,
            Self::NotifyCancellation => 7,
            Self::Enter(node) => 8_u8.checked_add(node as u8).ok_or(MachineError::Capacity)?,
        };
        let mut identity = context.event.bytes();
        identity[31] = code;
        intents.stage(ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(identity),
                digest: Digest::of(&[code]).0,
                payload_bytes: 1,
            },
            payload: self,
        })
    }
}
struct Order;
const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 1,
};
impl HierarchyModel for Order {
    type State = OrderState;
    type Event = OrderEvent;
    type Effect = OrderEffect;
    type Error = MachineError;
    type Node = Node;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn leaf(&self, state: &OrderState) -> Node {
        match state {
            OrderState::Open(Open::Draft) => Node::Draft,
            OrderState::Open(Open::AwaitingPayment) => Node::AwaitingPayment,
            OrderState::Paid { .. } => Node::Paid,
            OrderState::Cancelled => Node::Cancelled,
        }
    }
    fn parent(&self, node: Node) -> Option<Node> {
        match node {
            Node::Root => None,
            Node::Open | Node::Paid | Node::Cancelled => Some(Node::Root),
            Node::Draft | Node::AwaitingPayment => Some(Node::Open),
        }
    }
    fn validate(&self, state: &OrderState) -> Result<(), MachineError> {
        if matches!(state, OrderState::Paid { receipt: 0 }) {
            Err(MachineError::InvalidHierarchy)
        } else {
            Ok(())
        }
    }
    fn select(
        &self,
        node: Node,
        _: &OrderState,
        event: &OrderEvent,
        _: &TransitionContext,
    ) -> Result<HierarchyDecision<Node>, MachineError> {
        let (target, transition) = match (node, event) {
            (Node::Draft, OrderEvent::Submit) => (Node::AwaitingPayment, 1),
            (Node::AwaitingPayment, OrderEvent::PaymentConfirmed(0)) => {
                return Ok(HierarchyDecision::Rejected {
                    reason: RejectCode::new(1),
                });
            }
            (Node::AwaitingPayment, OrderEvent::PaymentConfirmed(_)) => (Node::Paid, 2),
            (Node::Paid, OrderEvent::RefreshReceipt) => (Node::Paid, 4),
            // The root policy applies unless a closed leaf explicitly refuses it.
            (Node::Root, OrderEvent::Cancel) => (Node::Cancelled, 3),
            (Node::Paid | Node::Cancelled, OrderEvent::Cancel) => {
                return Ok(HierarchyDecision::Rejected {
                    reason: RejectCode::new(2),
                });
            }
            _ => return Ok(HierarchyDecision::Unhandled),
        };
        Ok(HierarchyDecision::Selected(HierarchyPlan {
            target,
            kind: TransitionKind::External,
            transition: TransitionId::new(transition),
        }))
    }
    fn candidate(
        &self,
        plan: HierarchyPlan<Node>,
        current: &OrderState,
        event: &OrderEvent,
        _: &TransitionContext,
    ) -> Result<OrderState, MachineError> {
        match (plan.target, event) {
            (Node::AwaitingPayment, OrderEvent::Submit) => {
                Ok(OrderState::Open(Open::AwaitingPayment))
            }
            (Node::Paid, OrderEvent::PaymentConfirmed(receipt)) => {
                Ok(OrderState::Paid { receipt: *receipt })
            }
            (Node::Paid, OrderEvent::RefreshReceipt) => match current {
                OrderState::Paid { receipt } => Ok(OrderState::Paid { receipt: *receipt }),
                OrderState::Open(_) | OrderState::Cancelled => Err(MachineError::InvalidHierarchy),
            },
            (Node::Cancelled, OrderEvent::Cancel) => Ok(OrderState::Cancelled),
            _ => Err(MachineError::InvalidHierarchy),
        }
    }
    fn exit<S: IntentSink<OrderEffect>>(
        &self,
        node: Node,
        _: &OrderState,
        _: &mut OrderState,
        context: &TransitionContext,
        intents: &mut S,
    ) -> Result<(), MachineError> {
        OrderEffect::Exit(node).stage(context, intents)
    }
    fn enter<S: IntentSink<OrderEffect>>(
        &self,
        node: Node,
        _: &OrderState,
        _: &mut OrderState,
        context: &TransitionContext,
        intents: &mut S,
    ) -> Result<(), MachineError> {
        OrderEffect::Enter(node).stage(context, intents)
    }

    fn action<S: IntentSink<OrderEffect>>(
        &self,
        plan: HierarchyPlan<Node>,
        _: &OrderState,
        _: &mut OrderState,
        context: &TransitionContext,
        intents: &mut S,
    ) -> Result<(), MachineError> {
        let effect = match plan.target {
            Node::AwaitingPayment => OrderEffect::RequestPayment,
            Node::Cancelled => OrderEffect::NotifyCancellation,
            Node::Root | Node::Open | Node::Draft | Node::Paid => return Ok(()),
        };
        effect.stage(context, intents)
    }
}
fn run(events: &[(u8, OrderEvent)]) -> Result<(), MachineFailure<HierarchyFailure<MachineError>>> {
    let scope = MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: DEFINITION,
    };
    let mut order = MemoryMachine::<_, 4>::new(
        Hierarchical::<_, 3>::new(Order),
        scope,
        OrderState::Open(Open::Draft),
        IntentLimits {
            max_intents: 4,
            max_payload_bytes: 4,
        },
    )?;
    for (ordinal, event) in events {
        let ordinal = *ordinal;
        // Fixture IDs/digest; real admission must bind the actual event and evidence.
        let context = TransitionContext {
            expected: order.read().0,
            event: MachineEventId::from_bytes([ordinal; 32]),
            event_digest: [ordinal; 32],
            logical_time: MachineLogicalTime::new(u64::from(ordinal)),
        };
        println!("source={:?} event={event:?}", order.read().1);
        match order.handle(event, context)? {
            HandleOutcome::Accepted(record) => println!(
                "Accepted revision={} transition={}",
                record.revision.get(),
                record.transition.get()
            ),
            HandleOutcome::Rejected { reason } => println!("Rejected guard={}", reason.get()),
            HandleOutcome::Unhandled => println!("Unhandled"),
        }
        println!("state={:?}", order.read().1);
        for intent in order.take_intents() {
            println!("intent={:?} (not dispatched)", intent.intent.payload);
        }
    }
    Ok(())
}

fn main() -> Result<(), MachineFailure<HierarchyFailure<MachineError>>> {
    run(&[
        (1, OrderEvent::Submit),
        (2, OrderEvent::PaymentConfirmed(0)), // Guard refuses; no hooks run.
        (3, OrderEvent::PaymentConfirmed(7)),
        (4, OrderEvent::RefreshReceipt), // External self-edge exits and reenters Paid.
        (5, OrderEvent::Cancel),         // Paid refuses; the parent cannot override it.
    ])?;
    run(&[(1, OrderEvent::Cancel)]) // Draft bubbles through Open to the root policy.
}
