#![no_main]
//! Canonical byte binding, bounded preparation, receipt substitution and abort.
use libfuzzer_sys::fuzz_target;
use penelope_core::{machine::*, task::Digest};
use penelope_ports::machine::*;

const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 1,
};
const SCOPE: MachineScope = MachineScope {
    tenant: MachineTenantId::from_bytes([1; 32]),
    instance: MachineId::from_bytes([2; 32]),
    definition: DEFINITION,
};
struct Model;
impl Machine for Model {
    type State = u64;
    type Event = u64;
    type Effect = u64;
    type Error = MachineError;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, state: &u64) -> Result<(), MachineError> {
        if *state > 1024 {
            Err(MachineError::Capacity)
        } else {
            Ok(())
        }
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &u64,
        event: &u64,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<u64>, MachineError> {
        let next = current + event;
        let intent = || ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(context.event.bytes()),
                digest: Digest::of(&next.to_le_bytes()).0,
                payload_bytes: 8,
            },
            payload: next,
        };
        sink.stage(intent())?;
        match event & 7 {
            0 => Ok(Transition::Rejected {
                reason: RejectCode::new(1),
            }),
            1 => Ok(Transition::Unhandled),
            2 => Err(MachineError::InvalidHierarchy),
            3 => Ok(Transition::Accepted {
                next: u64::MAX,
                transition: TransitionId::new(1),
            }),
            4 => {
                let _ignored = sink.stage(intent());
                Ok(Transition::Accepted {
                    next,
                    transition: TransitionId::new(1),
                })
            }
            _ => Ok(Transition::Accepted {
                next,
                transition: TransitionId::new(1),
            }),
        }
    }
}
struct Codec(u8);
fn write(value: u64, output: &mut [u8]) -> Result<usize, ()> {
    output
        .get_mut(..8)
        .ok_or(())?
        .copy_from_slice(&value.to_le_bytes());
    Ok(8)
}
fn read(input: &[u8]) -> Result<u64, ()> {
    Ok(u64::from_le_bytes(input.try_into().map_err(|_| ())?))
}
impl MachineCodec<Model> for Codec {
    type Error = ();
    fn definition(&self) -> MachineDefinition {
        if self.0 == 4 {
            MachineDefinition {
                state_schema: 2,
                ..DEFINITION
            }
        } else {
            DEFINITION
        }
    }
    fn event_type(&self, _: &u64) -> MachineEventType {
        MachineEventType::new(1)
    }
    fn encode_state(&self, state: &u64, output: &mut [u8]) -> Result<usize, ()> {
        if self.0 == 2 {
            return Ok(usize::MAX);
        }
        if self.0 == 5 {
            return Err(());
        }
        write(*state, output)
    }
    fn decode_state(&self, input: &[u8]) -> Result<u64, ()> {
        read(input)
    }
    fn encode_event(&self, event: &u64, output: &mut [u8]) -> Result<usize, ()> {
        write(if self.0 == 1 { event + 1 } else { *event }, output)
    }
    fn decode_event(&self, kind: MachineEventType, input: &[u8]) -> Result<u64, ()> {
        if kind != MachineEventType::new(1) {
            return Err(());
        }
        read(input)
    }
    fn encode_effect(&self, effect: &u64, output: &mut [u8]) -> Result<usize, ()> {
        if self.0 == 6 {
            return Ok(usize::MAX);
        }
        write(if self.0 == 3 { effect + 1 } else { *effect }, output)
    }
    fn decode_effect(&self, input: &[u8]) -> Result<u64, ()> {
        read(input)
    }
}
fn run<const N: usize>(data: &[u8]) {
    let byte = |index| data.get(index).copied().unwrap_or(0);
    let state = u64::from(byte(0));
    let event = u64::from(byte(1));
    let revision = if byte(2) & 1 == 0 { 0 } else { u64::MAX };
    let head = MachineHead {
        scope: SCOPE,
        revision: MachineRevision::new(revision),
    };
    let mut owner = MemoryMachine::<_, N>::restore(
        Model,
        head,
        state,
        IntentLimits {
            max_intents: N,
            max_payload_bytes: 32,
        },
    )
    .unwrap();
    let mut context = TransitionContext {
        expected: head,
        event: MachineEventId::from_bytes([4; 32]),
        event_digest: Digest::of(&event.to_le_bytes()).0,
        logical_time: MachineLogicalTime::new(u64::from(byte(3))),
    };
    if byte(2) & 2 != 0 {
        context.expected.revision = MachineRevision::new(if revision == 0 { 1 } else { 0 });
    }
    if byte(2) & 4 != 0 {
        context.expected.scope.tenant = MachineTenantId::from_bytes([99; 32]);
    }
    if byte(2) & 8 != 0 {
        context.event_digest = [99; 32];
    }
    let codec = Codec(byte(4) % 7);
    let limits = MachinePayloadLimits {
        max_record_bytes: 401,
        max_event_bytes: 8,
        max_state_bytes: 8,
        max_intent_bytes: 8,
        max_batch_bytes: if byte(5) & 1 == 0 { 425 } else { 424 },
    };
    let mut state_buffer = [0; 8];
    let mut event_buffer = [0; 8];
    let mut intent_buffer = [0; 8];
    let decision = prepare_machine_commit(
        &mut owner,
        &MachineCommitInput {
            event: &event,
            context,
            codec: &codec,
            causation: None,
        },
        &mut MachineCommitBuffers {
            state: &mut state_buffer,
            event: &mut event_buffer,
            intent: &mut intent_buffer,
        },
        limits,
    );
    let expected = N > 0 && event & 7 >= 5 && byte(2) & 15 == 0 && codec.0 == 0 && byte(5) & 1 == 0;
    let mut published = false;
    match decision {
        Ok(MachineCommitDecision::Prepared(prepared)) => {
            assert!(expected);
            let batch = prepared.batch();
            assert_eq!(batch.byte_len(), 425);
            assert_eq!(batch.event(), &event.to_le_bytes());
            assert_eq!(batch.state(), &(state + event).to_le_bytes());
            assert_eq!(
                batch.record().header().previous_state_digest,
                Digest::of(&state.to_le_bytes()).0
            );
            assert_eq!(
                batch.record().header().next_state_digest,
                Digest::of(batch.state()).0
            );
            for (metadata, payload) in batch.intents() {
                assert_eq!(metadata.digest, Digest::of(payload).0);
                assert_eq!(metadata.payload_bytes, payload.len() as u64);
            }
            let mut wire = vec![0; batch.record().encoded_len()];
            batch.record().encode(&mut wire).unwrap();
            let recovered = MachineStoredCommit::<N>::decode(
                &wire,
                batch.event(),
                batch.state(),
                batch.intents().map(|(_, b)| b),
                limits,
            )
            .unwrap();
            assert_eq!(recovered.batch().byte_len(), batch.byte_len());
            let restored = recovered
                .restore(
                    Model,
                    &Codec(0),
                    SCOPE,
                    &mut [0; 8],
                    IntentLimits {
                        max_intents: N,
                        max_payload_bytes: 32,
                    },
                )
                .unwrap();
            assert_eq!(*restored.read().1, state + event);
            assert_eq!(restored.pending_intents().count(), 0);
            assert_eq!(recovered.receipt().identity, batch.record().identity());
            let mut corrupt = batch.state().to_vec();
            corrupt[usize::from(byte(8)) % 8] ^= 1;
            assert!(
                MachineStoredCommit::<N>::decode(
                    &wire,
                    batch.event(),
                    &corrupt,
                    batch.intents().map(|(_, b)| b),
                    limits,
                )
                .is_err()
            );
            let record = batch.record().header().transition;
            let mut receipt = MachineCommitReceipt {
                head: MachineHead {
                    scope: SCOPE,
                    revision: record.revision,
                },
                event: context.event,
                identity: batch.record().identity(),
                disposition: if byte(6) & 1 == 0 {
                    MachineCommitDisposition::Committed
                } else {
                    MachineCommitDisposition::AlreadyCommitted
                },
            };
            let variant = byte(7) % 10;
            match variant {
                0 => {}
                1 => receipt.head.revision = MachineRevision::ZERO,
                2 => receipt.head.scope.tenant = MachineTenantId::from_bytes([99; 32]),
                3 => receipt.head.scope.instance = MachineId::from_bytes([99; 32]),
                4 => {
                    receipt.head.scope.definition.fingerprint =
                        MachineDefinitionId::from_bytes([99; 32])
                }
                5 => receipt.head.scope.definition.state_schema = 99,
                6 => receipt.head.scope.definition.event_schema = 99,
                7 => receipt.event = MachineEventId::from_bytes([99; 32]),
                8 => receipt.identity = MachineCommitId::from_bytes([99; 32]),
                _ => {
                    drop(prepared);
                    assert_eq!(owner.read(), (head, &state));
                    return;
                }
            }
            let result = prepared.publish(receipt);
            published = variant == 0;
            assert_eq!(result.is_ok(), published);
        }
        Ok(MachineCommitDecision::Rejected { .. } | MachineCommitDecision::Unhandled) | Err(_) => {
            assert!(!expected)
        }
    }
    if published {
        assert_eq!(
            owner.read(),
            (
                MachineHead {
                    scope: SCOPE,
                    revision: MachineRevision::new(1)
                },
                &(state + event)
            )
        );
        assert_eq!(owner.pending_intents().count(), 1);
    } else {
        assert_eq!(owner.read(), (head, &state));
        assert_eq!(owner.pending_intents().count(), 0);
        assert!(owner.last_transition().is_none());
    }
}
fuzz_target!(|data: &[u8]| {
    let limits = MachinePayloadLimits {
        max_record_bytes: 2048,
        max_event_bytes: 64,
        max_state_bytes: 64,
        max_intent_bytes: 64,
        max_batch_bytes: 4096,
    };
    // Arbitrary framing must remain bounded, even with an infinite payload source.
    let _ = MachineStoredCommit::<4>::decode(data, data, data, core::iter::repeat(data), limits);
    match data.first().copied().unwrap_or(0) % 3 {
        0 => run::<0>(data),
        1 => run::<1>(data),
        _ => run::<4>(data),
    }
});
