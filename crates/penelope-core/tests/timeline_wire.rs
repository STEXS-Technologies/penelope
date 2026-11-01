//! Independent golden protocol bytes, strict parsing and trusted checkpoint restoration.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
use penelope_core::{task::Digest, timeline::*};

const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
fn plain() -> TimelineHead {
    TimelineHead::new(
        scope(),
        TimelineBranchId::new(7),
        TimelineStep::new(6),
        TimelineRevision::new(9),
        None,
    )
    .unwrap()
}
const fn observation() -> AuthorityObservation {
    AuthorityObservation {
        scope: scope(),
        authority: TimelineAuthorityId::from_digest(Digest([10; 32])),
        epoch: AuthorityEpoch::new(2),
        sequence: AuthoritySequence::new(0x0102_0304_0506_0708),
        step: TimelineStep::new(3),
        digest: Digest([9; 32]),
    }
}
fn full() -> TimelineHead {
    let baseline = TimelineHead::new(
        scope(),
        TimelineBranchId::new(7),
        TimelineStep::new(6),
        TimelineRevision::new(8),
        Some(TimelineStep::new(1)),
    )
    .unwrap();
    let mut projection = TimelineProjection::<0>::new(baseline);
    projection.observe(baseline, observation()).unwrap()
}
const fn input() -> InputRecord {
    InputRecord {
        scope: scope(),
        branch: TimelineBranchId::new(7),
        step: TimelineStep::new(4),
        identity: TimelineInputId::from_digest(Digest([5; 32])),
        revision: InputRevision::new(0x0102_0304_0506_0708),
        digest: Digest([6; 32]),
    }
}
const fn anchor() -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch: TimelineBranchId::new(7),
        step: TimelineStep::new(3),
        snapshot: TimelineSnapshotId::from_digest(Digest([8; 32])),
        digest: Digest([9; 32]),
    }
}
fn fixtures() -> [(TimelineRecord, &'static str); 7] {
    [
        (
            TimelineRecord::Checkpoint {
                head: plain(),
                last_branch: TimelineBranchId::new(9),
            },
            include_str!("fixtures/timeline_v1/checkpoint-empty.hex"),
        ),
        (
            TimelineRecord::Checkpoint {
                head: full(),
                last_branch: TimelineBranchId::new(9),
            },
            include_str!("fixtures/timeline_v1/checkpoint-full.hex"),
        ),
        (
            TimelineRecord::Input(input()),
            include_str!("fixtures/timeline_v1/input.hex"),
        ),
        (
            TimelineRecord::Anchor(anchor()),
            include_str!("fixtures/timeline_v1/anchor.hex"),
        ),
        (
            TimelineRecord::Fork {
                expected: full(),
                branch: TimelineBranchId::new(8),
                anchor: anchor(),
                replacement: None,
            },
            include_str!("fixtures/timeline_v1/fork-state.hex"),
        ),
        (
            TimelineRecord::Fork {
                expected: full(),
                branch: TimelineBranchId::new(8),
                anchor: anchor(),
                replacement: Some(input()),
            },
            include_str!("fixtures/timeline_v1/fork-input.hex"),
        ),
        (
            TimelineRecord::Intent(TimelineIntentRecord {
                scope: scope(),
                branch: TimelineBranchId::new(8),
                step: TimelineStep::new(4),
                revision: TimelineRevision::new(10),
                identity: TimelineEffectId::from_digest(Digest([11; 32])),
                digest: Digest([12; 32]),
                payload_bytes: 12,
            }),
            include_str!("fixtures/timeline_v1/intent.hex"),
        ),
    ]
}
fn hex(text: &str) -> Vec<u8> {
    let text: String = text.split_whitespace().collect();
    (0..text.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
        .collect()
}

#[test]
fn independently_assembled_v1_golden_records_are_canonical() {
    let mut maximum = 0;
    for (record, fixture) in fixtures() {
        let golden = hex(fixture);
        let mut output = [0xAA; TIMELINE_RECORD_MAX_BYTES + 1];
        let written = record.encode(&mut output).unwrap();
        assert_eq!(written, record.encoded_len());
        assert_eq!(&output[..written], golden);
        assert_eq!(output[written], 0xAA);
        assert_eq!(TimelineRecord::decode(&golden).unwrap(), record);
        maximum = maximum.max(written);
        let mut too_short = vec![0xAA; written - 1];
        assert_eq!(
            record.encode(&mut too_short),
            Err(TimelineWireError::Capacity)
        );
        assert!(too_short.iter().all(|byte| *byte == 0xAA));
    }
    assert_eq!(maximum, TIMELINE_RECORD_MAX_BYTES);
}
#[test]
fn every_truncation_unknown_tag_flag_and_trailing_byte_is_refused() {
    for (record, fixture) in fixtures() {
        let golden = hex(fixture);
        for count in 0..golden.len() {
            assert!(TimelineRecord::decode(&golden[..count]).is_err());
        }
        let mut trailing = golden.clone();
        trailing.push(0);
        assert_eq!(
            TimelineRecord::decode(&trailing),
            Err(TimelineWireError::InvalidFormat)
        );
        let mut unknown = golden.clone();
        unknown[19] = b'2';
        assert_eq!(
            TimelineRecord::decode(&unknown),
            Err(TimelineWireError::UnsupportedVersion)
        );
        unknown[19] = b'1';
        unknown[20] = 0;
        assert_eq!(
            TimelineRecord::decode(&unknown),
            Err(TimelineWireError::InvalidFormat)
        );
        if matches!(
            record,
            TimelineRecord::Checkpoint { .. } | TimelineRecord::Fork { .. }
        ) {
            let mut flag = golden.clone();
            flag[173] = 2; // Prefix21 + scope128 + branch/step/revision24.
            assert_eq!(
                TimelineRecord::decode(&flag),
                Err(TimelineWireError::InvalidFormat)
            );
        }
    }
}
#[test]
fn checkpoint_restoration_preserves_authority_finality_and_consumed_branch_ids() {
    let bytes = hex(include_str!("fixtures/timeline_v1/checkpoint-full.hex"));
    let TimelineRecord::Checkpoint { head, last_branch } = TimelineRecord::decode(&bytes).unwrap()
    else {
        panic!("expected a checkpoint fixture");
    };
    let mut projection = TimelineProjection::<1>::restore(head, last_branch).unwrap();
    assert_eq!(projection.head(), full());
    assert_eq!(projection.last_branch(), TimelineBranchId::new(9));
    assert_eq!(
        projection.observe(head, observation()),
        Err(TimelineError::StaleAuthority)
    );
    let original = [
        InputRecord {
            step: TimelineStep::new(4),
            ..input()
        },
        InputRecord {
            step: TimelineStep::new(5),
            ..input()
        },
        InputRecord {
            step: TimelineStep::new(6),
            ..input()
        },
    ];
    let plan = ReplayPlan::prepare(
        head,
        head.revision(),
        Correction::State(anchor()),
        &original,
        ReplayLimits {
            max_retained_inputs: 3,
            max_replay_steps: 3,
        },
    )
    .unwrap();
    // The already accepted observation can drive its own correction after restore.
    let reservation = projection.reserve(&plan, observation()).unwrap();
    assert_eq!(reservation.branch(), TimelineBranchId::new(10));
    assert_eq!(
        projection.head().effect_finalized(),
        Some(TimelineStep::new(1))
    );
}
#[test]
fn malformed_frontiers_scope_and_fork_bindings_fail_closed() {
    let full_fixture = include_str!("fixtures/timeline_v1/checkpoint-full.hex");
    let mut future = hex(full_fixture);
    future[174..182].copy_from_slice(&7_u64.to_le_bytes());
    assert_eq!(
        TimelineRecord::decode(&future),
        Err(TimelineError::InvalidFrontier.into())
    );
    let mut wrong_scope = hex(full_fixture);
    wrong_scope[183] ^= 1; // Authority's repeated scope starts after the presence flag.
    assert_eq!(
        TimelineRecord::decode(&wrong_scope),
        Err(TimelineError::ScopeMismatch.into())
    );
    let mut low_water = hex(full_fixture);
    let last = low_water.len() - 8;
    low_water[last..].copy_from_slice(&6_u64.to_le_bytes());
    assert_eq!(
        TimelineRecord::decode(&low_water),
        Err(TimelineError::Conflict.into())
    );
    // A wire checkpoint may carry exhausted counters, but it cannot authorize a new fork.
    let exhausted = TimelineHead::new(
        scope(),
        full().branch(),
        full().step(),
        TimelineRevision::new(u64::MAX),
        None,
    )
    .unwrap();
    let variants = [
        (
            TimelineRecord::Fork {
                expected: exhausted,
                branch: TimelineBranchId::new(8),
                anchor: anchor(),
                replacement: None,
            },
            TimelineError::CounterExhausted,
        ),
        (
            TimelineRecord::Fork {
                expected: full(),
                branch: TimelineBranchId::new(8),
                anchor: StateAnchor {
                    branch: TimelineBranchId::new(99),
                    ..anchor()
                },
                replacement: None,
            },
            TimelineError::ScopeMismatch,
        ),
        (
            TimelineRecord::Fork {
                expected: full(),
                branch: full().branch(),
                anchor: anchor(),
                replacement: None,
            },
            TimelineError::Conflict,
        ),
        (
            TimelineRecord::Fork {
                expected: full(),
                branch: TimelineBranchId::new(8),
                anchor: StateAnchor {
                    step: TimelineStep::ZERO,
                    ..anchor()
                },
                replacement: None,
            },
            TimelineError::ReconciliationRequired,
        ),
        (
            TimelineRecord::Fork {
                expected: full(),
                branch: TimelineBranchId::new(8),
                anchor: StateAnchor {
                    step: TimelineStep::new(7),
                    ..anchor()
                },
                replacement: None,
            },
            TimelineError::FutureCorrection,
        ),
        (
            TimelineRecord::Fork {
                expected: full(),
                branch: TimelineBranchId::new(8),
                anchor: anchor(),
                replacement: Some(InputRecord {
                    step: TimelineStep::new(5),
                    ..input()
                }),
            },
            TimelineError::InputMismatch,
        ),
    ];
    for (record, error) in variants {
        let mut output = [0xCC; TIMELINE_RECORD_MAX_BYTES];
        assert_eq!(record.encode(&mut output), Err(error.into()));
        assert_eq!(output, [0xCC; TIMELINE_RECORD_MAX_BYTES]);
    }
}

#[test]
fn optional_zero_frontier_and_full_width_counters_round_trip_without_coercion() {
    for finalized in [None, Some(TimelineStep::ZERO), Some(TimelineStep::new(6))] {
        for authority in [false, true] {
            let base = TimelineHead::new(
                scope(),
                TimelineBranchId::new(7),
                TimelineStep::new(6),
                TimelineRevision::new(8),
                finalized,
            )
            .unwrap();
            let mut projection = TimelineProjection::<0>::new(base);
            let current = if authority {
                projection.observe(base, observation()).unwrap()
            } else {
                base
            };
            let checkpoint = TimelineRecord::Checkpoint {
                head: current,
                last_branch: TimelineBranchId::new(9),
            };
            let mut bytes = [0; TIMELINE_RECORD_MAX_BYTES];
            let len = checkpoint.encode(&mut bytes).unwrap();
            assert_eq!(TimelineRecord::decode(&bytes[..len]).unwrap(), checkpoint);
        }
    }
    let maximum_head = TimelineHead::new(
        scope(),
        TimelineBranchId::new(u64::MAX),
        TimelineStep::new(u64::MAX),
        TimelineRevision::new(u64::MAX - 1),
        Some(TimelineStep::new(u64::MAX)),
    )
    .unwrap();
    let mut projection = TimelineProjection::<0>::new(maximum_head);
    let observed = AuthorityObservation {
        epoch: AuthorityEpoch::new(u64::MAX),
        sequence: AuthoritySequence::new(u64::MAX),
        step: TimelineStep::new(u64::MAX),
        ..observation()
    };
    let maximum_head = projection.observe(maximum_head, observed).unwrap();
    let records = [
        TimelineRecord::Checkpoint {
            head: maximum_head,
            last_branch: TimelineBranchId::new(u64::MAX),
        },
        TimelineRecord::Input(InputRecord {
            branch: TimelineBranchId::new(u64::MAX),
            step: TimelineStep::new(u64::MAX),
            revision: InputRevision::new(u64::MAX),
            ..input()
        }),
        TimelineRecord::Anchor(StateAnchor {
            branch: TimelineBranchId::new(u64::MAX),
            step: TimelineStep::new(u64::MAX),
            ..anchor()
        }),
        TimelineRecord::Intent(TimelineIntentRecord {
            scope: scope(),
            branch: TimelineBranchId::new(u64::MAX),
            step: TimelineStep::new(u64::MAX),
            revision: TimelineRevision::new(u64::MAX),
            identity: TimelineEffectId::from_digest(Digest([0xFF; 32])),
            digest: Digest([0xFF; 32]),
            payload_bytes: u64::MAX,
        }),
    ];
    for record in records {
        let mut bytes = [0; TIMELINE_RECORD_MAX_BYTES];
        let len = record.encode(&mut bytes).unwrap();
        assert_eq!(TimelineRecord::decode(&bytes[..len]).unwrap(), record);
    }
    let mut malformed = hex(include_str!("fixtures/timeline_v1/fork-input.hex"));
    malformed[615] = 2;
    assert_eq!(
        TimelineRecord::decode(&malformed),
        Err(TimelineWireError::InvalidFormat)
    );
    let mut future_authority = hex(include_str!("fixtures/timeline_v1/checkpoint-full.hex"));
    future_authority[359..367].copy_from_slice(&7_u64.to_le_bytes());
    assert_eq!(
        TimelineRecord::decode(&future_authority),
        Err(TimelineError::FutureCorrection.into())
    );
}
