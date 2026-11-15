//! Exact candidate/payload binding, aborts and receipt substitution boundaries.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn
)]
use penelope_core::{machine::*, task::Digest};
use penelope_ports::machine::*;
use std::sync::Arc;

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
const LIMITS: MachinePayloadLimits = MachinePayloadLimits {
    max_record_bytes: 4096,
    max_event_bytes: 8,
    max_state_bytes: 8,
    max_intent_bytes: 8,
    max_batch_bytes: 1024,
};
#[derive(Debug, PartialEq, Eq)]
struct State(u64); // Deliberately not Clone or serializable.
#[derive(Debug, PartialEq, Eq)]
enum Fault {
    Evaluation,
    Invalid,
    Staging(MachineError),
}
struct Model {
    bad_charge: bool,
    bad_digest: bool,
    silent: bool,
}
impl Machine for Model {
    type State = State;
    type Event = u64;
    type Effect = u64;
    type Error = Fault;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, state: &State) -> Result<(), Fault> {
        if state.0 > 1024 {
            Err(Fault::Invalid)
        } else {
            Ok(())
        }
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &State,
        event: &u64,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<State>, Fault> {
        let next = current.0 + event;
        if self.silent {
            return Ok(Transition::Accepted {
                next: State(next),
                transition: TransitionId::new(7),
            });
        }
        sink.stage(ProposedIntent {
            metadata: IntentMetadata {
                identity: MachineIntentId::from_bytes(context.event.bytes()),
                digest: if self.bad_digest {
                    [99; 32]
                } else {
                    Digest::of(&next.to_le_bytes()).0
                },
                payload_bytes: if self.bad_charge { 7 } else { 8 },
            },
            payload: next,
        })
        .map_err(Fault::Staging)?;
        match *event {
            0 => Ok(Transition::Rejected {
                reason: RejectCode::new(1),
            }),
            1 => Ok(Transition::Unhandled),
            2 => Err(Fault::Evaluation),
            3 => Ok(Transition::Accepted {
                next: State(1025),
                transition: TransitionId::new(7),
            }),
            _ => Ok(Transition::Accepted {
                next: State(next),
                transition: TransitionId::new(7),
            }),
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
enum CodecFault {
    Refused,
    Capacity,
    Malformed,
}
struct Codec(u8);
fn write(value: u64, output: &mut [u8]) -> Result<usize, CodecFault> {
    output
        .get_mut(..8)
        .ok_or(CodecFault::Capacity)?
        .copy_from_slice(&value.to_le_bytes());
    Ok(8)
}
fn read(input: &[u8]) -> Result<u64, CodecFault> {
    let bytes: [u8; 8] = input.try_into().map_err(|_size| CodecFault::Malformed)?;
    Ok(u64::from_le_bytes(bytes))
}
impl MachineCodec<Model> for Codec {
    type Error = CodecFault;
    fn definition(&self) -> MachineDefinition {
        if self.0 == 1 {
            MachineDefinition {
                event_schema: 2,
                ..DEFINITION
            }
        } else {
            DEFINITION
        }
    }
    fn event_type(&self, _: &u64) -> MachineEventType {
        MachineEventType::new(17)
    }
    fn encode_state(&self, state: &State, output: &mut [u8]) -> Result<usize, CodecFault> {
        if self.0 == 8 && state.0 > 5 {
            panic!("candidate codec panic");
        }
        if self.0 == 2 || (self.0 == 7 && state.0 > 5) {
            return Err(CodecFault::Refused);
        }
        write(state.0, output)
    }
    fn decode_state(&self, input: &[u8]) -> Result<State, CodecFault> {
        Ok(State(read(input)? + u64::from(self.0 == 11)))
    }
    fn encode_event(&self, event: &u64, output: &mut [u8]) -> Result<usize, CodecFault> {
        if self.0 == 9 {
            panic!("event codec panic");
        }
        if self.0 == 3 {
            return Ok(usize::MAX);
        }
        write(if self.0 == 4 { event + 1 } else { *event }, output)
    }
    fn decode_event(&self, kind: MachineEventType, input: &[u8]) -> Result<u64, CodecFault> {
        if kind != MachineEventType::new(17) {
            return Err(CodecFault::Malformed);
        }
        read(input)
    }
    fn encode_effect(&self, effect: &u64, output: &mut [u8]) -> Result<usize, CodecFault> {
        if self.0 == 10 {
            panic!("effect codec panic");
        }
        if self.0 == 6 {
            return Ok(9);
        }
        write(if self.0 == 5 { effect + 1 } else { *effect }, output)
    }
    fn decode_effect(&self, input: &[u8]) -> Result<u64, CodecFault> {
        read(input)
    }
}
#[derive(Default)]
struct Scratch {
    state: [u8; 8],
    event: [u8; 8],
    intent: [u8; 8],
}
impl Scratch {
    const fn buffers(&mut self) -> MachineCommitBuffers<'_> {
        MachineCommitBuffers {
            state: &mut self.state,
            event: &mut self.event,
            intent: &mut self.intent,
        }
    }
}
fn owner(model: Model) -> MemoryMachine<Model, 2> {
    MemoryMachine::new(
        model,
        SCOPE,
        State(5),
        IntentLimits {
            max_intents: 2,
            max_payload_bytes: 16,
        },
    )
    .unwrap()
}
const fn model() -> Model {
    Model {
        bad_charge: false,
        bad_digest: false,
        silent: false,
    }
}
fn context(head: MachineHead, event: u64) -> TransitionContext {
    TransitionContext {
        expected: head,
        event: MachineEventId::from_bytes([4; 32]),
        event_digest: Digest::of(&event.to_le_bytes()).0,
        logical_time: MachineLogicalTime::new(19),
    }
}
const fn receipt<const N: usize>(
    batch: &MachineCommitBatch<N>,
    disposition: MachineCommitDisposition,
) -> MachineCommitReceipt {
    let record = batch.record().header().transition;
    MachineCommitReceipt {
        head: MachineHead {
            scope: record.context.expected.scope,
            revision: record.revision,
        },
        event: record.context.event,
        identity: batch.record().identity(),
        disposition,
    }
}

#[test]
fn accepted_non_clone_state_retains_exact_bytes_and_only_publishes_after_matching_receipt() {
    for disposition in [
        MachineCommitDisposition::Committed,
        MachineCommitDisposition::AlreadyCommitted,
    ] {
        let causation = if disposition == MachineCommitDisposition::AlreadyCommitted {
            Some(MachineCausation {
                source: MachineSourceId::from_bytes([8; 32]),
                cause: MachineCauseId::from_bytes([9; 32]),
                correlation: MachineCorrelationId::from_bytes([10; 32]),
            })
        } else {
            None
        };
        let mut owner = owner(model());
        let context = context(owner.read().0, 8);
        let mut scratch = Scratch::default();
        let decision = prepare_machine_commit(
            &mut owner,
            &MachineCommitInput {
                event: &8,
                context,
                codec: &Codec(0),
                causation,
            },
            &mut scratch.buffers(),
            LIMITS,
        )
        .unwrap();
        let MachineCommitDecision::Prepared(prepared) = decision else {
            panic!("missing candidate");
        };
        let batch = prepared.batch();
        let shared = prepared.batch();
        assert_eq!(prepared.before(), Some(&State(5)));
        assert_eq!(prepared.candidate(), &State(13));
        assert_eq!(prepared.provenance(), &batch.record().header().transition);
        assert_eq!(prepared.intents().count(), 1);
        assert!(Arc::ptr_eq(&batch, &shared));
        assert_eq!(batch.event(), &8_u64.to_le_bytes());
        assert_eq!(batch.state(), &13_u64.to_le_bytes());
        assert_eq!(
            batch.byte_len(),
            if causation.is_some() { 521 } else { 425 }
        );
        let header = batch.record().header();
        assert_eq!(header.causation, causation);
        assert_eq!(header.transition.context, context);
        assert_eq!(header.event_type, MachineEventType::new(17));
        assert_eq!(
            header.previous_state_digest,
            Digest::of(&5_u64.to_le_bytes()).0
        );
        assert_eq!(header.next_state_digest, Digest::of(batch.state()).0);
        let intents: Vec<_> = batch.intents().collect();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].0.digest, Digest::of(intents[0].1).0);
        assert_eq!(intents[0].1, &13_u64.to_le_bytes());
        let record = prepared.publish(receipt(&batch, disposition)).unwrap();
        assert_eq!(
            owner.read(),
            (
                MachineHead {
                    scope: SCOPE,
                    revision: MachineRevision::new(1)
                },
                &State(13)
            )
        );
        assert_eq!(owner.last_transition(), Some(record));
        assert_eq!(owner.pending_intents().count(), 1);
        // Arc payloads remain immutable after native handle consumption.
        assert_eq!(shared.record().header().transition, record);
    }
}

#[test]
fn drop_and_every_receipt_coordinate_substitution_abort_local_publication() {
    for variant in 0..9 {
        let mut owner = owner(model());
        let before = owner.read().0;
        let context = context(before, 8);
        let mut scratch = Scratch::default();
        let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
            &mut owner,
            &MachineCommitInput {
                event: &8,
                context,
                codec: &Codec(0),
                causation: None,
            },
            &mut scratch.buffers(),
            LIMITS,
        )
        .unwrap() else {
            panic!("missing candidate");
        };
        let batch = prepared.batch();
        let mut receipt = receipt(&batch, MachineCommitDisposition::Committed);
        match variant {
            0 => receipt.head.scope.tenant = MachineTenantId::from_bytes([99; 32]),
            1 => receipt.head.scope.instance = MachineId::from_bytes([99; 32]),
            2 => {
                receipt.head.scope.definition.fingerprint =
                    MachineDefinitionId::from_bytes([99; 32])
            }
            3 => receipt.head.scope.definition.state_schema = 2,
            4 => receipt.head.scope.definition.event_schema = 2,
            5 => receipt.head.revision = MachineRevision::new(2),
            6 => receipt.event = MachineEventId::from_bytes([99; 32]),
            7 => receipt.identity = MachineCommitId::from_bytes([99; 32]),
            _ => {
                drop(prepared);
                assert_eq!(owner.read(), (before, &State(5)));
                continue;
            }
        }
        assert_eq!(prepared.publish(receipt), Err(MachineReceiptError));
        assert_eq!(owner.read(), (before, &State(5)));
        assert_eq!(owner.last_transition(), None);
        assert_eq!(owner.pending_intents().count(), 0);
        // Retained exact requests support lookup/retry after local cancellation;
        // their existence is not evidence that a store actually committed them.
        assert_eq!(batch.state(), &13_u64.to_le_bytes());
    }
}

#[test]
fn model_refusal_failure_and_invalid_candidate_never_produce_a_commit_batch() {
    for event in 0..4 {
        let mut owner = owner(model());
        let before = owner.read().0;
        let mut scratch = Scratch::default();
        let result = prepare_machine_commit(
            &mut owner,
            &MachineCommitInput {
                event: &event,
                context: context(before, event),
                codec: &Codec(0),
                causation: None,
            },
            &mut scratch.buffers(),
            LIMITS,
        );
        match event {
            0 => assert!(
                matches!(result, Ok(MachineCommitDecision::Rejected { reason }) if reason == RejectCode::new(1))
            ),
            1 => assert!(matches!(result, Ok(MachineCommitDecision::Unhandled))),
            2 => assert!(matches!(
                result,
                Err(MachinePreparationError::Machine(MachineFailure::Model(
                    Fault::Evaluation
                )))
            )),
            _ => assert!(matches!(
                result,
                Err(MachinePreparationError::Machine(MachineFailure::Model(
                    Fault::Invalid
                )))
            )),
        }
        assert_eq!(owner.read(), (before, &State(5)));
        assert_eq!(owner.pending_intents().count(), 0);
    }
}

#[test]
fn codec_definition_length_digest_and_intent_charge_failures_leave_owner_unchanged() {
    for mode in 1..10 {
        let mut model = model();
        model.bad_charge = mode == 8;
        model.bad_digest = mode == 9;
        let mut owner = owner(model);
        let before = owner.read().0;
        let mut scratch = Scratch::default();
        let codec = Codec(if mode < 8 { mode } else { 0 });
        let result = prepare_machine_commit(
            &mut owner,
            &MachineCommitInput {
                event: &8,
                context: context(before, 8),
                codec: &codec,
                causation: None,
            },
            &mut scratch.buffers(),
            LIMITS,
        );
        match mode {
            1 => assert!(matches!(
                result,
                Err(MachinePreparationError::Machine(MachineFailure::Kernel(
                    MachineError::DefinitionMismatch
                )))
            )),
            2 | 7 => assert!(matches!(
                result,
                Err(MachinePreparationError::Codec(CodecFault::Refused))
            )),
            3 | 6 => assert!(matches!(result, Err(MachinePreparationError::Capacity))),
            _ => assert!(matches!(
                result,
                Err(MachinePreparationError::PayloadMismatch)
            )),
        }
        assert_eq!(owner.read(), (before, &State(5)));
        assert_eq!(owner.pending_intents().count(), 0);
    }
}

#[test]
fn every_byte_limit_and_input_digest_is_enforced_before_local_publication() {
    for variant in 0..8 {
        let mut owner = owner(model());
        let before = owner.read().0;
        let mut scratch = Scratch::default();
        let mut limits = LIMITS;
        let mut context = context(before, 8);
        match variant {
            0 => limits.max_record_bytes = 400,
            1 => limits.max_batch_bytes = 424,
            2 => limits.max_event_bytes = 7,
            3 => limits.max_state_bytes = 7,
            4 => limits.max_intent_bytes = 7,
            5 => context.event_digest = [99; 32],
            6 => context.expected.revision = MachineRevision::new(1),
            _ => context.expected.scope.instance = MachineId::from_bytes([99; 32]),
        }
        assert!(
            prepare_machine_commit(
                &mut owner,
                &MachineCommitInput {
                    event: &8,
                    context,
                    codec: &Codec(0),
                    causation: None
                },
                &mut scratch.buffers(),
                limits
            )
            .is_err()
        );
        assert_eq!(owner.read(), (before, &State(5)));
        assert_eq!(owner.pending_intents().count(), 0);
    }
    // Exact limits accept, rather than requiring an extra byte of slack.
    let mut owner = owner(model());
    let context = context(owner.read().0, 8);
    let mut scratch = Scratch::default();
    let exact = MachinePayloadLimits {
        max_record_bytes: 401,
        max_batch_bytes: 425,
        ..LIMITS
    };
    assert!(matches!(
        prepare_machine_commit(
            &mut owner,
            &MachineCommitInput {
                event: &8,
                context,
                codec: &Codec(0),
                causation: None
            },
            &mut scratch.buffers(),
            exact
        ),
        Ok(MachineCommitDecision::Prepared(_))
    ));
}

#[test]
fn codec_reference_rejects_wrong_type_trailing_bytes_and_preserves_non_clone_values() {
    let codec = Codec(0);
    assert_eq!(codec.decode_state(&5_u64.to_le_bytes()).unwrap(), State(5));
    assert_eq!(
        codec
            .decode_event(MachineEventType::new(17), &8_u64.to_le_bytes())
            .unwrap(),
        8
    );
    assert_eq!(codec.decode_effect(&13_u64.to_le_bytes()).unwrap(), 13);
    assert_eq!(codec.decode_state(&[0; 9]), Err(CodecFault::Malformed));
    assert_eq!(
        codec.decode_event(MachineEventType::new(18), &[0; 8]),
        Err(CodecFault::Malformed)
    );
    assert_eq!(codec.decode_effect(&[0; 7]), Err(CodecFault::Malformed));
}

// A one-event in-memory test peer, deliberately not a crash-durable adapter.
struct Journal {
    head: MachineHead,
    state_digest: [u8; 32],
    committed: Option<Arc<MachineCommitBatch<2>>>,
}
impl Journal {
    fn new() -> Self {
        Self {
            head: MachineHead {
                scope: SCOPE,
                revision: MachineRevision::ZERO,
            },
            state_digest: Digest::of(&5_u64.to_le_bytes()).0,
            committed: None,
        }
    }
    fn commit(
        &mut self,
        batch: Arc<MachineCommitBatch<2>>,
    ) -> Result<MachineCommitReceipt, &'static str> {
        let header = batch.record().header();
        if header.transition.context.expected.scope != self.head.scope {
            return Err("scope");
        }
        if let Some(old) = &self.committed
            && old.record().header().transition.context.event == header.transition.context.event
        {
            if old.record().identity() != batch.record().identity() {
                return Err("event conflict");
            }
            return Ok(receipt(old, MachineCommitDisposition::AlreadyCommitted));
        }
        if header.transition.context.expected != self.head {
            return Err("stale predecessor");
        }
        if header.previous_state_digest != self.state_digest {
            return Err("state predecessor");
        }
        let result = receipt(&batch, MachineCommitDisposition::Committed);
        self.head = result.head;
        self.state_digest = header.next_state_digest;
        self.committed = Some(batch);
        Ok(result)
    }
}

#[test]
fn lost_ack_retains_exact_request_and_requires_authoritative_lookup_before_recovery() {
    let mut owner = owner(model());
    let original = owner.read().0;
    let mut scratch = Scratch::default();
    let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
        &mut owner,
        &MachineCommitInput {
            event: &8,
            context: context(original, 8),
            codec: &Codec(0),
            causation: None,
        },
        &mut scratch.buffers(),
        LIMITS,
    )
    .unwrap() else {
        panic!("missing candidate");
    };
    let request = prepared.batch();
    let mut journal = Journal::new();
    let unobserved = journal.commit(Arc::clone(&request)).unwrap();
    // The store accepted it, but the caller loses the ACK and cancels its handle.
    drop(prepared);
    assert_eq!(owner.read(), (original, &State(5)));
    let recovered = journal.commit(Arc::clone(&request)).unwrap();
    assert_eq!(recovered.identity, unobserved.identity);
    assert_eq!(
        recovered.disposition,
        MachineCommitDisposition::AlreadyCommitted
    );
    let committed = journal.committed.as_ref().unwrap();
    assert_eq!(committed.record().identity(), request.record().identity());
    let state = Codec(0).decode_state(committed.state()).unwrap();
    let restored = MemoryMachine::<_, 2>::restore(
        model(),
        journal.head,
        state,
        IntentLimits {
            max_intents: 2,
            max_payload_bytes: 16,
        },
    )
    .unwrap();
    assert_eq!(restored.read(), (recovered.head, &State(13)));
    // Pending durable intents remain with the journal, not inferred by memory restore.
    assert_eq!(restored.pending_intents().count(), 0);
    assert_eq!(committed.intents().count(), 1);
}

#[test]
fn store_peer_checks_scoped_cas_state_predecessor_and_exact_duplicate_content() {
    for variant in 0..4 {
        let mut journal = Journal::new();
        if variant < 2 {
            let mut first = owner(model());
            let initial = first.read().0;
            let mut scratch = Scratch::default();
            let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
                &mut first,
                &MachineCommitInput {
                    event: &8,
                    context: context(initial, 8),
                    codec: &Codec(0),
                    causation: None,
                },
                &mut scratch.buffers(),
                LIMITS,
            )
            .unwrap() else {
                panic!("missing candidate");
            };
            journal.commit(prepared.batch()).unwrap();
        }
        let before = (
            journal.head,
            journal.state_digest,
            journal
                .committed
                .as_ref()
                .map(|batch| batch.record().identity()),
        );
        let mut scope = SCOPE;
        if variant == 3 {
            scope.tenant = MachineTenantId::from_bytes([99; 32]);
        }
        let mut next = MemoryMachine::<_, 2>::new(
            model(),
            scope,
            State(if variant == 2 { 6 } else { 5 }),
            IntentLimits {
                max_intents: 2,
                max_payload_bytes: 16,
            },
        )
        .unwrap();
        let mut context = context(next.read().0, 9);
        if variant == 1 {
            context.event = MachineEventId::from_bytes([55; 32]);
        }
        let mut scratch = Scratch::default();
        let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
            &mut next,
            &MachineCommitInput {
                event: &9,
                context,
                codec: &Codec(0),
                causation: None,
            },
            &mut scratch.buffers(),
            LIMITS,
        )
        .unwrap() else {
            panic!("missing candidate");
        };
        let expected = match variant {
            0 => "event conflict",
            1 => "stale predecessor",
            2 => "state predecessor",
            _ => "scope",
        };
        assert_eq!(journal.commit(prepared.batch()), Err(expected));
        assert_eq!(
            (
                journal.head,
                journal.state_digest,
                journal
                    .committed
                    .as_ref()
                    .map(|batch| batch.record().identity())
            ),
            before
        );
    }
}

#[test]
fn codec_panics_before_and_after_evaluation_leave_the_original_owner_coherent() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    for phase in [8, 9, 10] {
        let mut owner = owner(model());
        let before = owner.read().0;
        let mut scratch = Scratch::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _candidate = prepare_machine_commit(
                &mut owner,
                &MachineCommitInput {
                    event: &8,
                    context: context(before, 8),
                    codec: &Codec(phase),
                    causation: None,
                },
                &mut scratch.buffers(),
                LIMITS,
            );
        }));
        assert!(result.is_err());
        assert_eq!(owner.read(), (before, &State(5)));
        assert_eq!(owner.last_transition(), None);
        assert_eq!(owner.pending_intents().count(), 0);
    }
}

#[test]
fn zero_intent_profile_needs_no_effect_buffer_and_uses_the_same_preparation() {
    let mut owner = MemoryMachine::<_, 0>::new(
        Model {
            silent: true,
            ..model()
        },
        SCOPE,
        State(5),
        IntentLimits {
            max_intents: 0,
            max_payload_bytes: 0,
        },
    )
    .unwrap();
    let context = context(owner.read().0, 8);
    let mut state = [0; 8];
    let mut event = [0; 8];
    let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
        &mut owner,
        &MachineCommitInput {
            event: &8,
            context,
            codec: &Codec(6),
            causation: None,
        },
        &mut MachineCommitBuffers {
            state: &mut state,
            event: &mut event,
            intent: &mut [],
        },
        MachinePayloadLimits {
            max_record_bytes: 329,
            max_batch_bytes: 345,
            max_intent_bytes: 0,
            ..LIMITS
        },
    )
    .unwrap() else {
        panic!("missing candidate");
    };
    let batch = prepared.batch();
    assert_eq!(batch.byte_len(), 345);
    assert_eq!(batch.intents().count(), 0);
    prepared
        .publish(receipt(&batch, MachineCommitDisposition::Committed))
        .unwrap();
    assert_eq!(owner.read().1, &State(13));
    assert_eq!(owner.pending_intents().count(), 0);
}

fn retained_batch() -> Arc<MachineCommitBatch<2>> {
    let mut machine = owner(model());
    let mut scratch = Scratch::default();
    let context = context(machine.read().0, 8);
    let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
        &mut machine,
        &MachineCommitInput {
            event: &8,
            context,
            codec: &Codec(0),
            causation: None,
        },
        &mut scratch.buffers(),
        LIMITS,
    )
    .unwrap() else {
        panic!("accepted fixture")
    };
    prepared.batch()
}
fn encoded_record(batch: &MachineCommitBatch<2>) -> Vec<u8> {
    let mut bytes = vec![0; batch.record().encoded_len()];
    batch.record().encode(&mut bytes).unwrap();
    bytes
}

#[test]
fn stored_bytes_reconstruct_exact_receipt_and_non_clone_snapshot_without_pending_effects() {
    let batch = retained_batch();
    let record = encoded_record(&batch);
    let stored = MachineStoredCommit::<2>::decode(
        &record,
        batch.event(),
        batch.state(),
        batch.intents().map(|(_, bytes)| bytes),
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        stored.receipt(),
        receipt(&batch, MachineCommitDisposition::AlreadyCommitted)
    );
    assert_eq!(stored.batch().byte_len(), batch.byte_len());
    assert_eq!(stored.batch().intents().count(), 1);
    let restored = stored
        .restore(
            model(),
            &Codec(0),
            SCOPE,
            &mut [0; 8],
            IntentLimits {
                max_intents: 2,
                max_payload_bytes: 16,
            },
        )
        .unwrap();
    assert_eq!(restored.read(), (stored.receipt().head, &State(13)));
    assert!(restored.last_transition().is_none());
    assert_eq!(restored.pending_intents().count(), 0);
}

#[test]
fn recovery_refuses_every_payload_byte_substitution_and_all_payload_count_errors() {
    let batch = retained_batch();
    let record = encoded_record(&batch);
    let intent = batch.intents().next().unwrap().1;
    for coordinate in 0..3 {
        for offset in 0..8 {
            let mut event = batch.event().to_vec();
            let mut state = batch.state().to_vec();
            let mut payload = intent.to_vec();
            match coordinate {
                0 => event[offset] ^= 1,
                1 => state[offset] ^= 1,
                _ => payload[offset] ^= 1,
            }
            assert_eq!(
                MachineStoredCommit::<2>::decode(
                    &record,
                    &event,
                    &state,
                    [payload.as_slice()],
                    LIMITS
                )
                .err(),
                Some(MachineRecoveryError::PayloadMismatch)
            );
        }
    }
    for len in 0..8 {
        assert!(
            MachineStoredCommit::<2>::decode(
                &record,
                &batch.event()[..len],
                batch.state(),
                [intent],
                LIMITS
            )
            .is_err()
        );
        assert!(
            MachineStoredCommit::<2>::decode(
                &record,
                batch.event(),
                &batch.state()[..len],
                [intent],
                LIMITS
            )
            .is_err()
        );
        assert!(
            MachineStoredCommit::<2>::decode(
                &record,
                batch.event(),
                batch.state(),
                [&intent[..len]],
                LIMITS
            )
            .is_err()
        );
    }
    assert!(
        MachineStoredCommit::<2>::decode(&record, batch.event(), batch.state(), [], LIMITS)
            .is_err()
    );
    assert!(
        MachineStoredCommit::<2>::decode(
            &record,
            batch.event(),
            batch.state(),
            [intent, intent],
            LIMITS
        )
        .is_err()
    );
    let calls = std::cell::Cell::new(0);
    let unbounded = std::iter::from_fn(|| {
        calls.set(calls.get() + 1);
        Some(intent)
    });
    assert!(
        MachineStoredCommit::<2>::decode(&record, batch.event(), batch.state(), unbounded, LIMITS)
            .is_err()
    );
    assert_eq!(calls.get(), 2); // One declared payload plus one bounded excess probe.
    for field in 0..5 {
        let mut limits = LIMITS;
        match field {
            0 => limits.max_record_bytes = record.len() - 1,
            1 => limits.max_event_bytes = 7,
            2 => limits.max_state_bytes = 7,
            3 => limits.max_intent_bytes = 7,
            _ => limits.max_batch_bytes = 424,
        }
        assert_eq!(
            MachineStoredCommit::<2>::decode(
                &record,
                batch.event(),
                batch.state(),
                [intent],
                limits
            )
            .err(),
            Some(MachineRecoveryError::Capacity)
        );
    }
    assert!(
        MachineStoredCommit::<0>::decode(&record, batch.event(), batch.state(), [intent], LIMITS)
            .is_err()
    );
}

#[test]
fn snapshot_restore_refuses_scope_codec_lossy_decoding_and_model_invariant_substitution() {
    let batch = retained_batch();
    let record = encoded_record(&batch);
    let stored = MachineStoredCommit::<2>::decode(
        &record,
        batch.event(),
        batch.state(),
        batch.intents().map(|(_, b)| b),
        LIMITS,
    )
    .unwrap();
    let limits = IntentLimits {
        max_intents: 2,
        max_payload_bytes: 16,
    };
    let wrong_scope = MachineScope {
        tenant: MachineTenantId::from_bytes([99; 32]),
        ..SCOPE
    };
    assert!(matches!(
        stored.restore(model(), &Codec(0), wrong_scope, &mut [0; 8], limits),
        Err(MachinePreparationError::Machine(MachineFailure::Kernel(
            MachineError::ScopeMismatch
        )))
    ));
    assert!(matches!(
        stored.restore(model(), &Codec(1), SCOPE, &mut [0; 8], limits),
        Err(MachinePreparationError::Machine(MachineFailure::Kernel(
            MachineError::DefinitionMismatch
        )))
    ));
    assert!(
        stored
            .restore(model(), &Codec(0), SCOPE, &mut [0; 7], limits)
            .is_err()
    );
    assert!(matches!(
        stored.restore(model(), &Codec(11), SCOPE, &mut [0; 8], limits),
        Err(MachinePreparationError::PayloadMismatch)
    ));
    // A valid content hash is not model validity or source authenticity.
    let invalid_state = 1025_u64.to_le_bytes();
    let mut header = batch.record().header();
    header.next_state_digest = Digest::of(&invalid_state).0;
    let invalid_record =
        MachineCommitRecord::<2>::new(header, batch.record().intents().copied()).unwrap();
    let mut wire = vec![0; invalid_record.encoded_len()];
    invalid_record.encode(&mut wire).unwrap();
    let invalid_stored = MachineStoredCommit::<2>::decode(
        &wire,
        batch.event(),
        &invalid_state,
        batch.intents().map(|(_, b)| b),
        LIMITS,
    )
    .unwrap();
    assert!(matches!(
        invalid_stored.restore(model(), &Codec(0), SCOPE, &mut [0; 8], limits),
        Err(MachinePreparationError::Machine(MachineFailure::Model(
            Fault::Invalid
        )))
    ));
}

#[test]
fn recovery_preserves_order_and_supports_exact_zero_capacity() {
    let batch = retained_batch();
    let payload = 99_u64.to_le_bytes();
    let mut metadata: Vec<_> = batch.record().intents().copied().collect();
    metadata.push(IntentMetadata {
        identity: MachineIntentId::from_bytes([55; 32]),
        digest: Digest::of(&payload).0,
        payload_bytes: 8,
    });
    let record = MachineCommitRecord::<2>::new(batch.record().header(), metadata).unwrap();
    let mut bytes = vec![0; record.encoded_len()];
    record.encode(&mut bytes).unwrap();
    let first = batch.intents().next().unwrap().1;
    let stored = MachineStoredCommit::<2>::decode(
        &bytes,
        batch.event(),
        batch.state(),
        [first, &payload],
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        stored.batch().intents().map(|(_, b)| b).collect::<Vec<_>>(),
        [first, &payload]
    );
    assert!(
        MachineStoredCommit::<2>::decode(
            &bytes,
            batch.event(),
            batch.state(),
            [&payload, first],
            LIMITS
        )
        .is_err()
    );
    let empty_record = MachineCommitRecord::<0>::new(batch.record().header(), []).unwrap();
    let mut empty_bytes = vec![0; empty_record.encoded_len()];
    empty_record.encode(&mut empty_bytes).unwrap();
    let empty_stored = MachineStoredCommit::<0>::decode(
        &empty_bytes,
        batch.event(),
        batch.state(),
        [],
        MachinePayloadLimits {
            max_record_bytes: 329,
            max_intent_bytes: 0,
            max_batch_bytes: 345,
            ..LIMITS
        },
    )
    .unwrap();
    assert_eq!(empty_stored.batch().byte_len(), 345);
    assert_eq!(empty_stored.batch().intents().count(), 0);
    let calls = std::cell::Cell::new(0);
    let endless = std::iter::from_fn(|| {
        calls.set(calls.get() + 1);
        Some(first)
    });
    assert!(
        MachineStoredCommit::<0>::decode(
            &empty_bytes,
            batch.event(),
            batch.state(),
            endless,
            LIMITS
        )
        .is_err()
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn bootstrap_and_snapshot_restore_preserve_native_invariants_without_clone() {
    let bootstrap =
        prepare_machine_bootstrap(&model(), &Codec(0), SCOPE, &State(5), &mut [0; 8], LIMITS)
            .unwrap();
    assert_eq!(bootstrap.scope(), SCOPE);
    assert_eq!(bootstrap.state(), 5_u64.to_le_bytes());
    assert!(matches!(
        prepare_machine_bootstrap(
            &model(),
            &Codec(0),
            SCOPE,
            &State(1025),
            &mut [0; 8],
            LIMITS
        ),
        Err(MachinePreparationError::Machine(MachineFailure::Model(
            Fault::Invalid
        )))
    ));
    assert!(
        prepare_machine_bootstrap(&model(), &Codec(1), SCOPE, &State(5), &mut [0; 8], LIMITS)
            .is_err()
    );
    assert!(
        prepare_machine_bootstrap(&model(), &Codec(0), SCOPE, &State(5), &mut [0; 7], LIMITS)
            .is_err()
    );
    let head = MachineHead {
        scope: SCOPE,
        revision: MachineRevision::ZERO,
    };
    let snapshot =
        MachineSnapshot::decode(head, bootstrap.state(), bootstrap.digest(), None, LIMITS).unwrap();
    let restored: MemoryMachine<_, 2> = snapshot
        .restore(
            model(),
            &Codec(0),
            SCOPE,
            &mut [0; 8],
            IntentLimits {
                max_intents: 2,
                max_payload_bytes: 16,
            },
        )
        .unwrap();
    assert_eq!(restored.read(), (head, &State(5)));
    assert!(snapshot.last().is_none());
    assert_eq!(
        MachineSnapshot::decode(
            MachineHead {
                revision: MachineRevision::new(1),
                ..head
            },
            bootstrap.state(),
            bootstrap.digest(),
            None,
            LIMITS
        )
        .err(),
        Some(MachineStoreError::Corrupt)
    );
    assert_eq!(
        MachineSnapshot::decode(head, bootstrap.state(), [99; 32], None, LIMITS).err(),
        Some(MachineStoreError::Corrupt)
    );
    assert_eq!(
        MachineSnapshot::decode(
            head,
            bootstrap.state(),
            bootstrap.digest(),
            None,
            MachinePayloadLimits {
                max_state_bytes: 7,
                ..LIMITS
            }
        )
        .err(),
        Some(MachineStoreError::Capacity)
    );
}
