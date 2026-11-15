//! Full source and receiver-policy coordinates bind native task admission.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
use penelope_core::{machine::*, task::*};
use penelope_ports::machine::*;
#[path = "../../penelope-executor/tests/support/tasks.rs"]
mod native;
fn source() -> MachineIntentSource {
    MachineIntentSource {
        head: MachineHead {
            scope: MachineScope {
                tenant: MachineTenantId::from_bytes([1; 32]),
                instance: MachineId::from_bytes([2; 32]),
                definition: MachineDefinition {
                    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
                    state_schema: 1,
                    event_schema: 2,
                },
            },
            revision: MachineRevision::new(7),
        },
        event: MachineEventId::from_bytes([4; 32]),
        commit: MachineCommitId::from_bytes([5; 32]),
        intent: IntentMetadata {
            identity: MachineIntentId::from_bytes([6; 32]),
            digest: Digest::of(b"input").0,
            payload_bytes: 5,
        },
    }
}
fn definition() -> TaskDefinition {
    native::definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure)
}
const CONSUMER: MachineSourceId = MachineSourceId::from_bytes([8; 32]);
#[test]
fn all_source_coordinates_bind_the_task_identity_without_transport_coordinates() {
    let original = source();
    let binding = MachineTaskBinding::new(original, CONSUMER, definition(), b"input", 1).unwrap();
    assert_eq!(
        binding,
        MachineTaskBinding::new(original, CONSUMER, definition(), b"input", 1).unwrap()
    );
    for field in 0..9 {
        let mut changed = original;
        match field {
            0 => changed.head.scope.tenant = MachineTenantId::from_bytes([9; 32]),
            1 => changed.head.scope.instance = MachineId::from_bytes([9; 32]),
            2 => {
                changed.head.scope.definition.fingerprint = MachineDefinitionId::from_bytes([9; 32])
            }
            3 => changed.head.scope.definition.state_schema = 9,
            4 => changed.head.scope.definition.event_schema = 9,
            5 => changed.head.revision = MachineRevision::new(9),
            6 => changed.event = MachineEventId::from_bytes([9; 32]),
            7 => changed.commit = MachineCommitId::from_bytes([9; 32]),
            _ => changed.intent.identity = MachineIntentId::from_bytes([9; 32]),
        }
        let next = MachineTaskBinding::new(changed, CONSUMER, definition(), b"input", 1).unwrap();
        assert_ne!(next.scope().task, binding.scope().task, "field {field}");
        assert_ne!(next.scope(), binding.scope());
    }
    let mut invalid = original;
    invalid.intent.payload_bytes = 4;
    assert_eq!(
        MachineTaskBinding::new(invalid, CONSUMER, definition(), b"input", 1),
        Err(TaskError::ScopeMismatch)
    );
    invalid = original;
    invalid.intent.digest = [0; 32];
    assert_eq!(
        MachineTaskBinding::new(invalid, CONSUMER, definition(), b"input", 1),
        Err(TaskError::ScopeMismatch)
    );
}
#[test]
fn native_receipt_validation_keeps_generation_sequence_schema_definition_and_original_tick() {
    let binding = MachineTaskBinding::new(source(), CONSUMER, definition(), b"input", 1).unwrap();
    let record = binding.propose(TaskTick::new(7)).unwrap();
    binding.validate(&record).unwrap();
    assert_eq!(record.sequence, TaskSequence::ZERO);
    assert_eq!(record.generation, TaskGeneration::ZERO);
    let other_tick = binding.propose(TaskTick::new(8)).unwrap();
    assert_ne!(
        binding.receipt_digest(&record).unwrap(),
        binding.receipt_digest(&other_tick).unwrap()
    );
    let mut changed = record;
    changed.sequence = TaskSequence::new(1);
    assert!(binding.validate(&changed).is_err());
    changed = record;
    changed.generation = TaskGeneration::new(1);
    assert!(binding.validate(&changed).is_err());
    let wrong_schema =
        MachineTaskBinding::new(source(), CONSUMER, definition(), b"input", 2).unwrap();
    assert!(wrong_schema.validate(&record).is_err());
    let mut changed_definition = definition();
    changed_definition.recovery.max_attempts = 2;
    let wrong_definition =
        MachineTaskBinding::new(source(), CONSUMER, changed_definition, b"input", 1).unwrap();
    assert!(wrong_definition.validate(&record).is_err());
}

#[test]
fn source_binding_matches_independent_sha256_fixed_width_golden() {
    // Python hashlib over the documented tag, fixed 32-byte identities and LE
    // u32 schemas/u64 counters; not computed with the production encoder.
    assert_eq!(
        source().binding_identity(CONSUMER),
        [
            0x93, 0x3e, 0x67, 0x1d, 0x09, 0xcb, 0x45, 0x26, 0x4b, 0x66, 0xc2, 0xc1, 0x83, 0x5d,
            0x6b, 0x83, 0x82, 0x37, 0x4c, 0xc6, 0x34, 0x34, 0x93, 0x2e, 0x2b, 0x67, 0x4c, 0xac,
            0x14, 0xb3, 0xb1, 0xca,
        ]
    );
}
