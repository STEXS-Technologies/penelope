//! A pure flat machine. Intents describe work; this example executes no I/O effects.
#[path = "support/inspection.rs"]
mod inspection;
use penelope::machine::*;

#[path = "support/counter.rs"]
mod counter;
use counter::{Counter, DEFINITION};

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
        inspection::inspect("machine", &ready);
        println!("candidate={} (not published)", ready.state());
        ready.publish();
    }
    let (head, state) = counter.read();
    println!("published revision={} state={state}", head.revision.get());
    for intent in counter.take_intents() {
        println!(
            "committed intent={} (not dispatched)",
            intent.intent.payload
        );
    }
    Ok(())
}
