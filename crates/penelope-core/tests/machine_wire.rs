//! Frozen independently encoded bytes, malformed records and canonical admission.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use penelope_core::{
    __private::{put_bytes, take_bytes},
    machine::*,
    timeline::TimelineRecord,
};

const EMPTY: &[u8] = include_bytes!("fixtures/machine-v1/empty.bin");
const CAUSED: &[u8] = include_bytes!("fixtures/machine-v1/caused.bin");

const fn header(caused: bool) -> MachineCommitHeader {
    MachineCommitHeader {
        transition: TransitionRecord {
            context: TransitionContext {
                expected: MachineHead {
                    scope: MachineScope {
                        tenant: MachineTenantId::from_bytes([1; 32]),
                        instance: MachineId::from_bytes([2; 32]),
                        definition: MachineDefinition {
                            fingerprint: MachineDefinitionId::from_bytes([3; 32]),
                            state_schema: 0x01020304,
                            event_schema: 0xa0b0c0d0,
                        },
                    },
                    revision: MachineRevision::new(if caused { u64::MAX - 1 } else { 0 }),
                },
                event: MachineEventId::from_bytes([4; 32]),
                event_digest: [5; 32],
                logical_time: MachineLogicalTime::new(if caused {
                    u64::MAX
                } else {
                    0x0102030405060708
                }),
            },
            revision: MachineRevision::new(if caused { u64::MAX } else { 1 }),
            transition: TransitionId::new(0x8877665544332211),
        },
        event_type: MachineEventType::new(0x1020304050607080),
        previous_state_digest: [6; 32],
        next_state_digest: [7; 32],
        causation: if caused {
            Some(MachineCausation {
                source: MachineSourceId::from_bytes([8; 32]),
                cause: MachineCauseId::from_bytes([9; 32]),
                correlation: MachineCorrelationId::from_bytes([10; 32]),
            })
        } else {
            None
        },
    }
}
const fn intent(id: u8, digest: u8, charge: u64) -> IntentMetadata {
    IntentMetadata {
        identity: MachineIntentId::from_bytes([id; 32]),
        digest: [digest; 32],
        payload_bytes: charge,
    }
}
fn encoded<const N: usize>(record: &MachineCommitRecord<N>) -> Vec<u8> {
    let mut bytes = vec![0; record.encoded_len()];
    assert_eq!(record.encode(&mut bytes).unwrap(), bytes.len());
    bytes
}

#[test]
fn independent_goldens_bind_endianness_causation_order_and_full_width_counters() {
    let empty = MachineCommitRecord::<0>::new(header(false), []).unwrap();
    assert_eq!(encoded(&empty), EMPTY);
    assert_eq!(empty.header(), header(false));
    assert!(empty.is_empty());
    assert_eq!(empty.payload_bytes(), 0);
    let expected = [intent(11, 12, 0), intent(13, 14, u64::MAX)];
    let caused = MachineCommitRecord::<2>::new(header(true), expected).unwrap();
    assert_eq!(encoded(&caused), CAUSED);
    assert_eq!(caused.len(), 2);
    assert_eq!(caused.payload_bytes(), u64::MAX);
    assert_eq!(caused.intents().copied().collect::<Vec<_>>(), expected);
    assert_eq!(MachineCommitRecord::decode(EMPTY).unwrap(), empty);
    assert_eq!(MachineCommitRecord::decode(CAUSED).unwrap(), caused);
    // A caller's storage capacity is not part of canonical content identity.
    let larger = MachineCommitRecord::<8>::decode(CAUSED).unwrap();
    assert_eq!(larger.identity(), caused.identity());
    assert_eq!(encoded(&larger), CAUSED);
    assert!(TimelineRecord::decode(EMPTY).is_err());
    assert!(TimelineRecord::decode(CAUSED).is_err());
}

#[test]
fn every_truncation_mutation_and_trailing_byte_fails_closed() {
    for fixture in [EMPTY, CAUSED] {
        for len in 0..fixture.len() {
            assert!(MachineCommitRecord::<2>::decode(&fixture[..len]).is_err());
        }
        for index in 0..fixture.len() {
            for mask in [1, 0x80, 0xff] {
                let mut changed = fixture.to_vec();
                changed[index] ^= mask;
                assert!(
                    MachineCommitRecord::<2>::decode(&changed).is_err(),
                    "offset {index}, mask {mask}"
                );
            }
        }
        let mut trailing = fixture.to_vec();
        trailing.push(0);
        assert_eq!(
            MachineCommitRecord::<2>::decode(&trailing),
            Err(MachineWireError::InvalidFormat)
        );
    }
}

#[test]
fn canonical_structure_is_checked_even_with_a_recomputed_content_hash() {
    use penelope_core::task::Digest;
    let rewrite = |mut bytes: Vec<u8>| {
        let end = bytes.len() - 32;
        let digest = Digest::of(&bytes[..end]);
        bytes[end..].copy_from_slice(&digest.0);
        bytes
    };
    let mut variant = EMPTY.to_vec();
    variant[19] = 0;
    assert_eq!(
        MachineCommitRecord::<2>::decode(&rewrite(variant)),
        Err(MachineWireError::InvalidFormat)
    );
    let mut flag = EMPTY.to_vec();
    flag[292] = 2;
    assert_eq!(
        MachineCommitRecord::<2>::decode(&rewrite(flag)),
        Err(MachineWireError::InvalidFormat)
    );
    let mut revision = EMPTY.to_vec();
    revision[204..212].copy_from_slice(&2_u64.to_le_bytes());
    assert_eq!(
        MachineCommitRecord::<2>::decode(&rewrite(revision)),
        Err(MachineWireError::InvalidRevision)
    );
    let mut count = EMPTY.to_vec();
    count[293..297].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        MachineCommitRecord::<2>::decode(&rewrite(count)),
        Err(MachineWireError::Capacity)
    );
    let mut duplicate = CAUSED.to_vec();
    // Causation consumes 96 bytes; first and second intent IDs are at 393/465.
    duplicate[465..497].copy_from_slice(&[11; 32]);
    assert_eq!(
        MachineCommitRecord::<2>::decode(&rewrite(duplicate)),
        Err(MachineWireError::DuplicateIntent)
    );
    let mut overflow = CAUSED.to_vec();
    overflow[457..465].copy_from_slice(&1_u64.to_le_bytes());
    assert_eq!(
        MachineCommitRecord::<2>::decode(&rewrite(overflow)),
        Err(MachineWireError::Capacity)
    );
    assert_eq!(
        MachineCommitRecord::<1>::decode(CAUSED),
        Err(MachineWireError::Capacity)
    );
}

#[test]
fn construction_bounds_iterators_revisions_intents_and_charges_before_encoding() {
    use std::cell::Cell;
    let calls = Cell::new(0);
    let unbounded = std::iter::from_fn(|| {
        calls.set(calls.get() + 1);
        Some(intent(11, 12, 0))
    });
    assert_eq!(
        MachineCommitRecord::<0>::new(header(false), unbounded),
        Err(MachineWireError::Capacity)
    );
    assert_eq!(calls.get(), 1);
    let mut wrong = header(false);
    wrong.transition.revision = MachineRevision::new(0);
    assert_eq!(
        MachineCommitRecord::<0>::new(wrong, []),
        Err(MachineWireError::InvalidRevision)
    );
    wrong.transition.context.expected.revision = MachineRevision::new(u64::MAX);
    assert_eq!(
        MachineCommitRecord::<0>::new(wrong, []),
        Err(MachineWireError::InvalidRevision)
    );
    assert_eq!(
        MachineCommitRecord::<2>::new(header(false), [intent(1, 2, 0), intent(1, 3, 0)]),
        Err(MachineWireError::DuplicateIntent)
    );
    assert_eq!(
        MachineCommitRecord::<2>::new(header(false), [intent(1, 2, u64::MAX), intent(2, 3, 1)]),
        Err(MachineWireError::Capacity)
    );
    let original =
        MachineCommitRecord::<2>::new(header(false), [intent(1, 2, 0), intent(2, 3, 1)]).unwrap();
    let reversed =
        MachineCommitRecord::<2>::new(header(false), [intent(2, 3, 1), intent(1, 2, 0)]).unwrap();
    assert_ne!(original.identity(), reversed.identity());
}

#[test]
fn short_encoding_preserves_output_and_shared_field_cursors_do_not_advance_on_failure() {
    let record = MachineCommitRecord::<2>::decode(CAUSED).unwrap();
    for len in 0..CAUSED.len() {
        let mut bytes = vec![0xa5; len];
        assert_eq!(record.encode(&mut bytes), Err(MachineWireError::Capacity));
        assert!(bytes.iter().all(|byte| *byte == 0xa5));
    }
    let mut bytes = vec![0xa5; CAUSED.len() + 16];
    record.encode(&mut bytes).unwrap();
    assert_eq!(&bytes[..CAUSED.len()], CAUSED);
    assert!(bytes[CAUSED.len()..].iter().all(|byte| *byte == 0xa5));
    let mut input: &[u8] = &[1, 2];
    assert_eq!(take_bytes::<3>(&mut input), None);
    assert_eq!(input, &[1, 2]);
    assert_eq!(take_bytes::<2>(&mut input), Some([1, 2]));
    assert!(input.is_empty());
    let mut output = [0; 2];
    let mut cursor = output.as_mut_slice();
    assert_eq!(put_bytes(&mut cursor, &[1, 2, 3]), None);
    assert_eq!(cursor.len(), 2);
    assert_eq!(put_bytes(&mut cursor, &[1, 2]), Some(()));
    assert!(cursor.is_empty());
    assert_eq!(output, [1, 2]);
}
