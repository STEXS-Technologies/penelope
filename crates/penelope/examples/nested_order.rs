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
enum OrderEvent {
    Submit,
    PaymentConfirmed(u64),
    Cancel,
}
#[derive(Clone, Copy, PartialEq, Eq)]
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
    RequestPayment,
    NotifyCancellation,
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
        let target = match (node, event) {
            (Node::Draft, OrderEvent::Submit) => Node::AwaitingPayment,
            (Node::AwaitingPayment, OrderEvent::PaymentConfirmed(0)) => {
                return Ok(HierarchyDecision::Rejected {
                    reason: RejectCode::new(1),
                });
            }
            (Node::AwaitingPayment, OrderEvent::PaymentConfirmed(_)) => Node::Paid,
            // Both open leaves bubble cancellation to their common parent.
            (Node::Open, OrderEvent::Cancel) => Node::Cancelled,
            (Node::Paid, OrderEvent::Cancel) => {
                return Ok(HierarchyDecision::Rejected {
                    reason: RejectCode::new(2),
                });
            }
            _ => return Ok(HierarchyDecision::Unhandled),
        };
        Ok(HierarchyDecision::Selected(HierarchyPlan {
            target,
            kind: TransitionKind::External,
            transition: TransitionId::new(match target {
                Node::AwaitingPayment => 1,
                Node::Paid => 2,
                Node::Cancelled => 3,
                Node::Root | Node::Open | Node::Draft => 0,
            }),
        }))
    }
    fn candidate(
        &self,
        plan: HierarchyPlan<Node>,
        _: &OrderState,
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
            (Node::Cancelled, OrderEvent::Cancel) => Ok(OrderState::Cancelled),
            _ => Err(MachineError::InvalidHierarchy),
        }
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
        intents.stage(ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(context.event.bytes()),
                digest: context.event_digest,
                payload_bytes: 1,
            },
            payload: effect,
        })
    }
}
fn main() -> Result<(), MachineFailure<HierarchyFailure<MachineError>>> {
    let scope = MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: DEFINITION,
    };
    let mut order = MemoryMachine::<_, 2>::new(
        Hierarchical::<_, 3>::new(Order),
        scope,
        OrderState::Open(Open::Draft),
        IntentLimits {
            max_intents: 2,
            max_payload_bytes: 2,
        },
    )?;
    for (ordinal, event) in [
        (1, OrderEvent::Submit),
        (2, OrderEvent::PaymentConfirmed(7)),
        (3, OrderEvent::Cancel),
    ] {
        // Fixture IDs/digest; real admission must bind the actual event and evidence.
        let context = TransitionContext {
            expected: order.read().0,
            event: MachineEventId::from_bytes([ordinal; 32]),
            event_digest: [ordinal; 32],
            logical_time: MachineLogicalTime::new(u64::from(ordinal)),
        };
        println!("decision={:?}", order.handle(&event, context)?);
        println!("state={:?}", order.read().1);
        for intent in order.take_intents() {
            println!("intent={:?} (not dispatched)", intent.intent.payload);
        }
    }
    Ok(())
}
