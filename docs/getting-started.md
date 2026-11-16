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
        event_digest: Digest::of(&event.to_le_bytes()).0,
        logical_time: MachineLogicalTime::new(1),
    };
    let outcome = counter.handle(&event, context)?;
    assert!(matches!(outcome, HandleOutcome::Accepted(_)));
    println!("count = {}", counter.read().1);
    Ok(())
}
```

Run `cargo run`. It prints `count = 3`.

`Counter` defines the transition. `handle` prepares, validates and publishes it in
one call. Its `HandleOutcome` distinguishes accepted, rejected and unhandled
events; errors return through `Result`. This example keeps state in memory and
performs no external effects or historical event deduplication.

Use `prepare` when you need to inspect or persist a candidate before publishing
it. Dropping the prepared handle leaves the current state unchanged.

The fixed IDs are demonstration values. In an application, assign stable tenant,
instance and event identities, pin the definition/schema identity, and bind each
event digest to its canonical bytes. Logical time is supplied by your application.

To stage work alongside a transition, see the
[counter example](../crates/penelope/examples/counter_machine.rs) and its
[shared model](../crates/penelope/examples/support/counter.rs). For persistence,
continue with the [machine storage contract](0.5/machine-storage.md) and
[durable consumer example](../ci/machine-consumer/durable.rs).

Run the durable progression with:

```sh
cargo run --manifest-path ci/machine-consumer/Cargo.toml --features durable --bin durable --locked
```

It rejects an overflowing transition without changing state, commits and restores
the counter, and checks the intent's originating event, commit and revision. It
then admits a native task, drops both database adapters before acknowledging the
source, and reopens them. After lease expiry, the new attempt recovers the exact
original task; the stale acknowledgement is refused. A second event starts a
native saga. Admission records work; a worker still owns execution and any
uncertain external result still requires reconciliation.

On Linux x86-64 with a C compiler, run the complete protected-worker progression:

```sh
./scripts/run-supervised-saga-example.sh
```

Its machine leg uses the same counter, recovery and admission code. A real
protected worker completes the admitted pure task; native journal replay, exact
output bytes, schema and application meaning are checked before its result is
committed as the counter's next event. The event retains task/source causation
and uses a stable result identity. Its new intent then starts a saga. The script
also runs a worker with the wrong result and verifies refusal before machine
admission. The optional `protected` consumer feature supplies the Linux runtime.
