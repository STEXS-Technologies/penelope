//! A flat enum model: one state enum, one event enum and one Machine implementation.
//! The key is example domain data; this local machine does not authenticate callers.
use penelope::machine::*;
use std::{convert::Infallible, error::Error};

#[derive(Debug, PartialEq, Eq)]
enum Door {
    Closed,
    Open,
    Locked { key: u64 },
}

#[derive(Debug)]
enum DoorEvent {
    Open,
    Close,
    Lock { key: u64 },
    Unlock { key: u64 },
}

struct DoorMachine;

impl Machine for DoorMachine {
    type State = Door;
    type Event = DoorEvent;
    type Effect = ();
    type Error = Infallible;

    fn definition(&self) -> MachineDefinition {
        MachineDefinition {
            fingerprint: MachineDefinitionId::from_bytes([1; 32]),
            state_schema: 1,
            event_schema: 1,
        }
    }

    fn validate(&self, _: &Door) -> Result<(), Infallible> {
        Ok(())
    }

    fn transition<S: IntentSink<()>>(
        &self,
        state: &Door,
        event: &DoorEvent,
        _: &TransitionContext,
        _: &mut S,
    ) -> Result<Transition<Door>, Infallible> {
        let (next, id) = match (state, event) {
            (Door::Closed, DoorEvent::Open) => (Door::Open, 1),
            (Door::Open, DoorEvent::Close) => (Door::Closed, 2),
            (Door::Closed, DoorEvent::Lock { key }) => (Door::Locked { key: *key }, 3),
            (Door::Locked { key: expected }, DoorEvent::Unlock { key }) => {
                if key != expected {
                    return Ok(Transition::Rejected {
                        reason: RejectCode::new(1),
                    });
                }
                (Door::Closed, 4)
            }
            (Door::Open, DoorEvent::Lock { .. }) => {
                return Ok(Transition::Rejected {
                    reason: RejectCode::new(2),
                });
            }
            (Door::Closed, DoorEvent::Close | DoorEvent::Unlock { .. })
            | (Door::Open, DoorEvent::Open | DoorEvent::Unlock { .. })
            | (Door::Locked { .. }, DoorEvent::Open | DoorEvent::Close | DoorEvent::Lock { .. }) => {
                return Ok(Transition::Unhandled);
            }
        };
        Ok(Transition::Accepted {
            next,
            transition: TransitionId::new(id),
        })
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let scope = MachineScope {
        tenant: MachineTenantId::from_bytes([2; 32]),
        instance: MachineId::from_bytes([3; 32]),
        definition: DoorMachine.definition(),
    };
    let mut door = MemoryMachine::<_, 0>::new(
        DoorMachine,
        scope,
        Door::Closed,
        IntentLimits {
            max_intents: 0,
            max_payload_bytes: 0,
        },
    )?;
    // Independent expected state, revision and decision after every event.
    // Ok(true) is accepted, Ok(false) unhandled, Err(code) a business refusal.
    for (ordinal, (event, state, revision, expected)) in (1_u8..).zip([
        (DoorEvent::Open, Door::Open, 1, Ok(true)),
        (DoorEvent::Lock { key: 7 }, Door::Open, 1, Err(2)),
        (DoorEvent::Close, Door::Closed, 2, Ok(true)),
        (
            DoorEvent::Lock { key: 7 },
            Door::Locked { key: 7 },
            3,
            Ok(true),
        ),
        (
            DoorEvent::Unlock { key: 8 },
            Door::Locked { key: 7 },
            3,
            Err(1),
        ),
        (DoorEvent::Open, Door::Locked { key: 7 }, 3, Ok(false)),
        (DoorEvent::Unlock { key: 7 }, Door::Closed, 4, Ok(true)),
        (DoorEvent::Close, Door::Closed, 4, Ok(false)),
    ]) {
        // Fixture coordinates for local execution. Durable preparation must bind
        // actual canonical event bytes through a codec instead of these values.
        let context = TransitionContext {
            expected: door.read().0,
            event: MachineEventId::from_bytes([ordinal; 32]),
            event_digest: [ordinal; 32],
            logical_time: MachineLogicalTime::new(u64::from(ordinal)),
        };
        let outcome = door.handle(&event, context)?;
        let decision = match outcome {
            HandleOutcome::Accepted(record) => {
                if record.context != context || record.transition.get() != revision {
                    return Err("accepted door provenance differs from the truth table".into());
                }
                Ok(true)
            }
            HandleOutcome::Rejected { reason } => Err(reason.get()),
            HandleOutcome::Unhandled => Ok(false),
        };
        if decision != expected
            || door.read().1 != &state
            || door.read().0.revision.get() != revision
        {
            return Err("door state, revision or decision differs from the truth table".into());
        }
        if door.pending_intents().next().is_some() {
            return Err("the zero-intent door model staged work".into());
        }
        println!("{event:?} -> {decision:?}; revision={revision} state={state:?}");
    }
    Ok(())
}
