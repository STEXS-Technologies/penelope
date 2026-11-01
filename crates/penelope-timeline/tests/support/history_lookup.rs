//! Linear immutable-version oracle independent of physical history slots.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
use penelope_core::task::Digest;
use penelope_timeline::*;

fn byte(data: &[u8], index: usize) -> u8 {
    data.get(index).copied().unwrap_or(0)
}
const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
fn record(branch: TimelineBranchId, step: u64, value: u64) -> InputRecord {
    InputRecord {
        scope: scope(),
        branch,
        step: TimelineStep::new(step),
        identity: TimelineInputId::from_digest(Digest::of(&step.to_le_bytes())),
        revision: InputRevision::ZERO,
        digest: value.digest(),
    }
}
pub fn run(data: &[u8]) {
    let count = usize::from(byte(data, 0) % 16) + 1;
    let expired = usize::from(byte(data, 1)) % (count + 1);
    let start = if byte(data, 0) & 128 != 0 {
        u64::MAX - 32
    } else {
        u64::from(byte(data, 2)) * 17
    };
    let branch = TimelineBranchId::new(u64::from(byte(data, 3)));
    let base = StateAnchor {
        scope: scope(),
        branch,
        step: TimelineStep::new(start),
        snapshot: TimelineSnapshotId::from_digest(Digest([4; 32])),
        digest: 0_u64.digest(),
    };
    let mut history = FixedHistory::<u64, u64, 64, 4, 2>::new(
        base,
        0,
        HistoryLimits {
            max_input_bytes: 8,
            max_snapshot_bytes: 8,
            max_total_bytes: 4096,
        },
    )
    .unwrap();
    let mut canonical = Vec::new();
    let mut versions = Vec::new();
    for i in 0..count {
        let value = u64::from(byte(data, i + 4)) * 257;
        let record = record(branch, start + i as u64 + 1, value);
        history.append_input(record, value).unwrap();
        canonical.push((record, value));
        versions.push((record, value));
        if byte(data, i + 20) & 1 != 0 {
            let replacement = value ^ 0xffff;
            let changed = InputRecord {
                revision: InputRevision::new(1),
                digest: replacement.digest(),
                ..record
            };
            history.import_input(changed, replacement).unwrap();
            versions.push((changed, replacement));
        }
    }
    let floor = TimelineStep::new(start + expired as u64);
    if expired != 0 {
        history
            .insert_snapshot(
                StateAnchor {
                    step: floor,
                    ..base
                },
                0,
            )
            .unwrap();
        history.expire_before(floor).unwrap();
    }
    // Reuse holes at the start while old canonical/imported entries remain.
    for i in 0..expired {
        let value = u64::from(byte(data, i + 40));
        let record = record(branch, start + count as u64 + i as u64 + 1, value);
        history.append_input(record, value).unwrap();
        canonical.push((record, value));
        versions.push((record, value));
    }
    for (record, value) in &versions {
        if record.step <= floor {
            assert!(matches!(
                history.read(*record),
                Err(TimelineError::HistoryUnavailable)
            ));
        } else {
            let read = history.read(*record).unwrap();
            assert_eq!(read.record, *record);
            assert_eq!(*read.value, *value);
            let mut changed = *record;
            changed.identity = TimelineInputId::from_digest(Digest([255; 32]));
            assert!(history.read(changed).is_err());
            changed = *record;
            changed.scope.schema = TimelineSchemaId::from_digest(Digest([254; 32]));
            assert!(matches!(
                history.read(changed),
                Err(TimelineError::ScopeMismatch)
            ));
        }
    }
    let retained: Vec<_> = canonical.iter().filter(|(r, _)| r.step > floor).collect();
    let mut scratch = [record(branch, start, 0); 32];
    let through = canonical.last().unwrap().0.step;
    let collected = history
        .collect_inputs(branch, floor, through, &mut scratch)
        .unwrap();
    assert_eq!(collected.len(), retained.len());
    for (actual, (expected, value)) in collected.iter().zip(retained) {
        assert_eq!(actual, expected);
        assert_eq!(history.input_at(branch, actual.step).unwrap(), *expected);
        assert_eq!(*history.read(*actual).unwrap().value, *value);
    }
    let bytes =
        versions.iter().filter(|(r, _)| r.step > floor).count() * 8 + history.anchor_count() * 8;
    assert_eq!(history.payload_bytes(), bytes as u64);
}
