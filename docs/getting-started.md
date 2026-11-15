# Your first machine

Install the branch dependency using the [README setup](../README.md#get-started).
Replace `src/main.rs` with this complete program:

```rust
use penelope::machine::*;

struct Counter;

const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([1; 32]),
    state_schema: 1,
    event_schema: 1,
};

impl Machine for Counter {
    type State = u64;
    type Event = u64;
    type Effect = ();
    type Error = MachineError;

    fn definition(&self) -> MachineDefinition { DEFINITION }

    fn validate(&self, _: &u64) -> Result<(), MachineError> { Ok(()) }

    fn transition<S: IntentSink<()>>(
        &self,
        current: &u64,
        amount: &u64,
        _: &TransitionContext,
        _: &mut S,
    ) -> Result<Transition<u64>, MachineError> {
        Ok(match current.checked_add(*amount) {
            Some(next) => Transition::Accepted {
                next,
                transition: TransitionId::new(1),
            },
            None => Transition::Rejected { reason: RejectCode::new(1) },
        })
    }
}

fn main() -> Result<(), MachineFailure<MachineError>> {
    let scope = MachineScope {
        tenant: MachineTenantId::from_bytes([2; 32]),
        instance: MachineId::from_bytes([3; 32]),
        definition: DEFINITION,
    };
    let mut counter = MemoryMachine::<_, 0>::new(
        Counter,
        scope,
        0,
        IntentLimits { max_intents: 0, max_payload_bytes: 0 },
    )?;
    let event = 3_u64;
    let context = TransitionContext {
        expected: counter.read().0,
        event: MachineEventId::from_bytes([4; 32]),
        event_digest: penelope::core::task::Digest::of(&event.to_le_bytes()).0,
        logical_time: MachineLogicalTime::new(1),
    };
    if let PrepareDecision::Ready(prepared) = counter.prepare(&event, context)? {
        prepared.publish();
    }
    println!("count = {}", counter.read().1);
    Ok(())
}
```

Run `cargo run`. It prints `count = 3`.

`Counter` defines the transition. `prepare` computes and validates a candidate;
`publish` makes it visible. Dropping the prepared handle leaves the current state
unchanged. This example keeps state in memory and performs no external effects.

The fixed IDs are demonstration values. In an application, assign stable tenant,
instance and event identities, pin the definition/schema identity, and bind each
event digest to its canonical bytes. Logical time is supplied by your application.

To stage work alongside a transition, see the
[counter example](../crates/penelope/examples/counter_machine.rs) and its
[shared model](../crates/penelope/examples/support/counter.rs). For persistence,
continue with the [machine storage contract](0.5/machine-storage.md) and
[durable consumer example](../ci/machine-consumer/durable.rs).
