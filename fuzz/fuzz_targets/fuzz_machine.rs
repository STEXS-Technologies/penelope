#![no_main]
//! State/revision/retained-intent oracle across failures, aborts and transfers.
use libfuzzer_sys::fuzz_target;
use penelope_core::machine::*;

const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 1,
};
fn scope() -> MachineScope {
    MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: DEFINITION,
    }
}
struct Event {
    opcode: u8,
    amount: u64,
}
#[derive(Debug, PartialEq, Eq)]
enum Fault {
    Evaluation,
    Invalid,
}
struct Counter;
impl Machine for Counter {
    type State = u64;
    type Event = Event;
    type Effect = u64;
    type Error = Fault;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, state: &u64) -> Result<(), Fault> {
        if *state > 1024 {
            Err(Fault::Invalid)
        } else {
            Ok(())
        }
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &u64,
        event: &Event,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<u64>, Fault> {
        let intent = || ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(context.event.bytes()),
                digest: context.event_digest,
                payload_bytes: event.amount % 16,
            },
            payload: *current,
        };
        let _ignored = sink.stage(intent());
        match event.opcode & 7 {
            0 => Ok(Transition::Rejected {
                reason: RejectCode::new(1),
            }),
            1 => Ok(Transition::Unhandled),
            2 => Err(Fault::Evaluation),
            3 => Ok(Transition::Accepted {
                next: 1025,
                transition: TransitionId::new(1),
            }),
            4 => {
                let _ignored = sink.stage(intent());
                Ok(Transition::Accepted {
                    next: current + event.amount,
                    transition: TransitionId::new(1),
                })
            }
            _ => Ok(Transition::Accepted {
                next: current + event.amount,
                transition: TransitionId::new(1),
            }),
        }
    }
}
fn run<const N: usize>(data: &[u8]) {
    let limit = usize::from(data.get(1).copied().unwrap_or(0)) % (N + 1);
    let max_bytes = u64::from(data.get(2).copied().unwrap_or(0));
    let head = MachineHead {
        scope: scope(),
        revision: MachineRevision::new(if data.first().copied().unwrap_or(0) & 8 == 0 {
            0
        } else {
            u64::MAX - 2
        }),
    };
    let mut owner = MemoryMachine::<_, N>::restore(
        Counter,
        head,
        0,
        IntentLimits {
            max_intents: limit,
            max_payload_bytes: max_bytes,
        },
    )
    .unwrap();
    let mut state = 0;
    let mut revision = head.revision.get();
    // Reference history is an ordinary Vec, independent of the fixed-slot publisher.
    let mut pending: Vec<([u8; 32], u64, u64, u64)> = Vec::new();
    for (index, bytes) in data.chunks(3).take(128).enumerate() {
        let opcode = bytes.first().copied().unwrap_or(0);
        let amount = u64::from(bytes.get(1).copied().unwrap_or(0));
        let charge = amount % 16;
        let mut id = [0; 32];
        id[..8].copy_from_slice(&(index as u64).to_be_bytes());
        if opcode & 64 != 0 {
            id = [99; 32];
        }
        let mut context = TransitionContext {
            expected: owner.read().0,
            event: MachineEventId::from_bytes(id),
            event_digest: id,
            logical_time: MachineLogicalTime::new(index as u64),
        };
        if opcode & 32 != 0 {
            context.expected.scope.instance = MachineId::from_bytes([99; 32]);
        }
        if opcode & 16 != 0 {
            context.expected.revision =
                MachineRevision::new(if revision == 0 { 1 } else { revision - 1 });
        }
        let charged: u64 = pending.iter().map(|entry| entry.1).sum();
        let kernel = if opcode & 32 != 0 {
            Some(MachineError::ScopeMismatch)
        } else if opcode & 16 != 0 {
            Some(MachineError::StaleHead)
        } else if pending.len() >= limit {
            Some(MachineError::Capacity)
        } else if pending.iter().any(|entry| entry.0 == id) {
            Some(MachineError::DuplicateIntent)
        } else if charged + charge > max_bytes {
            Some(MachineError::Capacity)
        } else if opcode & 7 == 4 {
            Some(if pending.len() + 1 >= limit {
                MachineError::Capacity
            } else {
                MachineError::DuplicateIntent
            })
        } else {
            None
        };
        let kind = opcode & 7;
        let accepted = kind >= 5 && state + amount <= 1024;
        let kernel = kernel.or(if accepted && revision == u64::MAX {
            Some(MachineError::RevisionExhausted)
        } else {
            None
        });
        let old_record = owner.last_transition();
        let result = owner.prepare(&Event { opcode, amount }, context);
        if let Some(expected) = kernel {
            assert!(matches!(result, Err(MachineFailure::Kernel(error)) if error == expected));
        } else if kind == 0 {
            assert!(
                matches!(result, Ok(PrepareDecision::Rejected { reason }) if reason == RejectCode::new(1))
            );
        } else if kind == 1 {
            assert!(matches!(result, Ok(PrepareDecision::Unhandled)));
        } else if kind == 2 {
            assert!(matches!(
                result,
                Err(MachineFailure::Model(Fault::Evaluation))
            ));
        } else if kind == 3 || state + amount > 1024 {
            assert!(matches!(result, Err(MachineFailure::Model(Fault::Invalid))));
        } else {
            let Ok(PrepareDecision::Ready(ready)) = result else {
                panic!("missing candidate");
            };
            assert_eq!(*ready.state(), state + amount);
            assert_eq!(ready.intents().count(), 1);
            if opcode & 8 == 0 {
                let record = ready.publish();
                revision += 1;
                assert_eq!(record.context, context);
                assert_eq!(record.revision.get(), revision);
                pending.push((id, charge, state, revision));
                state += amount;
            }
        }
        assert_eq!(*owner.read().1, state);
        assert_eq!(owner.read().0.revision.get(), revision);
        assert_eq!(
            owner
                .pending_intents()
                .map(|entry| (
                    entry.intent.metadata.identity.bytes(),
                    entry.intent.metadata.payload_bytes,
                    entry.intent.payload,
                    entry.origin.revision.get()
                ))
                .collect::<Vec<_>>(),
            pending
        );
        if !accepted || kernel.is_some() || opcode & 8 != 0 {
            assert_eq!(owner.last_transition(), old_record);
        }
        if opcode & 128 != 0 {
            let batch = owner.take_intents();
            assert_eq!(batch.len(), pending.len());
            assert_eq!(
                batch.payload_bytes(),
                pending.iter().map(|entry| entry.1).sum()
            );
            pending.clear();
        }
    }
}
fuzz_target!(|data: &[u8]| {
    match data.first().copied().unwrap_or(0) % 3 {
        0 => run::<0>(data),
        1 => run::<1>(data),
        _ => run::<4>(data),
    }
});
