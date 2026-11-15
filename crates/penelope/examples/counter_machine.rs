//! A pure flat machine. Intents describe work; this example executes no I/O effects.
use penelope::machine::*;

struct Counter;
const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 1,
};
impl Machine for Counter {
    type State = u64;
    type Event = u64;
    type Effect = u64;
    type Error = MachineError;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, _: &u64) -> Result<(), MachineError> {
        Ok(())
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &u64,
        amount: &u64,
        context: &TransitionContext,
        intents: &mut S,
    ) -> Result<Transition<u64>, MachineError> {
        if *amount == 0 {
            return Ok(Transition::Unhandled);
        }
        let Some(next) = current.checked_add(*amount) else {
            return Ok(Transition::Rejected {
                reason: RejectCode::new(1),
            });
        };
        intents.stage(ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(context.event.bytes()),
                digest: context.event_digest,
                payload_bytes: 8,
            },
            payload: next,
        })?;
        Ok(Transition::Accepted {
            next,
            transition: TransitionId::new(1),
        })
    }
}
fn main() -> Result<(), MachineFailure<MachineError>> {
    let scope = MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: DEFINITION,
    };
    let mut counter = MemoryMachine::<_, 2>::new(
        Counter,
        scope,
        0,
        IntentLimits {
            max_intents: 2,
            max_payload_bytes: 16,
        },
    )?;
    // Fixture IDs/digest for this local demo; a durable codec must bind actual bytes.
    let context = TransitionContext {
        expected: counter.read().0,
        event: MachineEventId::from_bytes([4; 32]),
        event_digest: [5; 32],
        logical_time: MachineLogicalTime::new(1),
    };
    if let PrepareDecision::Ready(ready) = counter.prepare(&3, context)? {
        println!("candidate={} (not published)", ready.state());
        ready.publish();
    }
    println!("published={:?}", counter.read());
    for intent in counter.take_intents() {
        println!(
            "committed intent={} (not dispatched)",
            intent.intent.payload
        );
    }
    Ok(())
}
