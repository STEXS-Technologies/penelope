//! Independent canonical-version oracle across nested corrections and child appends.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
use penelope_core::task::Digest;
use penelope_timeline::*;
fn byte(data: &[u8], i: usize) -> u8 {
    data.get(i).copied().unwrap_or(0)
}
const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
fn anchor(branch: TimelineBranchId, step: usize, state: u64) -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch,
        step: TimelineStep::new(step as u64),
        snapshot: TimelineSnapshotId::from_digest(state.digest()),
        digest: state.digest(),
    }
}
fn input(branch: TimelineBranchId, step: usize, value: u64) -> InputRecord {
    InputRecord {
        scope: scope(),
        branch,
        step: TimelineStep::new(step as u64),
        identity: TimelineInputId::from_digest((step as u64).digest()),
        revision: InputRevision::ZERO,
        digest: value.digest(),
    }
}
type History = FixedHistory<u64, u64, 128, 32, 8>;
fn check(history: &History, branch: TimelineBranchId, expected: &[(InputRecord, u64)]) {
    let mut scratch = [input(branch, 0, 0); 32];
    let actual = history
        .collect_inputs(
            branch,
            TimelineStep::ZERO,
            TimelineStep::new(expected.len() as u64),
            &mut scratch,
        )
        .unwrap();
    assert_eq!(actual.len(), expected.len());
    for (record, (wanted, value)) in actual.iter().zip(expected) {
        assert_eq!(record, wanted);
        assert_eq!(history.input_at(branch, record.step).unwrap(), *wanted);
        let bound = history.read(*record).unwrap();
        assert_eq!(bound.record, *wanted);
        assert_eq!(*bound.value, *value);
    }
}
pub fn run(data: &[u8]) {
    let root = TimelineBranchId::ZERO;
    let mut history = History::new(
        anchor(root, 0, 0),
        0,
        HistoryLimits {
            max_input_bytes: 8,
            max_snapshot_bytes: 8,
            max_total_bytes: 4096,
        },
    )
    .unwrap();
    let mut selected = Vec::new();
    for i in 0..usize::from(byte(data, 0) % 6) + 1 {
        let value = u64::from(byte(data, i + 3));
        let record = input(root, i + 1, value);
        history.append_input(record, value).unwrap();
        selected.push((record, value));
    }
    let mut projection = TimelineProjection::<1>::new(
        TimelineHead::new(
            scope(),
            root,
            TimelineStep::new(selected.len() as u64),
            TimelineRevision::ZERO,
            None,
        )
        .unwrap(),
    );
    let mut branches = vec![(root, selected.clone())];
    let mut imported = Vec::new();
    for generation in 0..usize::from(byte(data, 1) % 5) + 1 {
        let current = projection.head();
        let position = usize::from(byte(data, generation + 10)) % selected.len();
        let changed = u64::from(byte(data, generation + 20)) + 1000;
        let replacing = byte(data, generation + 30) & 1 != 0;
        let mut next = selected.clone();
        let correction = if replacing {
            let base = anchor(current.branch(), position, 0);
            // A complete canonical predecessor snapshot is admitted by the consumer.
            history.insert_snapshot(base, 0).unwrap();
            let previous = selected[position].0;
            let replacement = InputRecord {
                revision: InputRevision::new(previous.revision.get() + 1),
                digest: changed.digest(),
                ..previous
            };
            history.import_input(replacement, changed).unwrap();
            imported.push((replacement, changed));
            next[position] = (replacement, changed);
            Correction::Input {
                anchor: base,
                replacement,
            }
        } else {
            let base = anchor(current.branch(), position, changed);
            history.import_anchor(base, changed).unwrap();
            Correction::State(base)
        };
        let records: Vec<_> = selected[position..].iter().map(|(r, _)| *r).collect();
        let plan = ReplayPlan::prepare(
            current,
            current.revision(),
            correction,
            &records,
            ReplayLimits {
                max_retained_inputs: 32,
                max_replay_steps: 32,
            },
        )
        .unwrap();
        let observed = AuthorityObservation {
            scope: scope(),
            authority: TimelineAuthorityId::from_digest(Digest([9; 32])),
            epoch: AuthorityEpoch::ZERO,
            sequence: AuthoritySequence::new(generation as u64 + 1),
            step: plan.corrected_step(),
            digest: plan.corrected_digest(),
        };
        let reservation = projection.reserve(&plan, observed).unwrap();
        history.fork(&plan, reservation).unwrap();
        let child = projection.promote(reservation).unwrap();
        for (record, _) in &mut next {
            record.branch = child.branch();
        }
        let value = u64::from(byte(data, generation + 40));
        let record = input(child.branch(), next.len() + 1, value);
        history.append_input(record, value).unwrap();
        projection.append(child, record).unwrap();
        next.push((record, value));
        // An unselected imported version must remain directly readable, while
        // inherited canonical resolution continues to select the oracle's version.
        let shadow = InputRecord {
            revision: InputRevision::new(next[position].0.revision.get() + 50),
            digest: (changed + 1).digest(),
            ..next[position].0
        };
        history.import_input(shadow, changed + 1).unwrap();
        imported.push((shadow, changed + 1));
        selected = next;
        branches.push((child.branch(), selected.clone()));
        for (branch, expected) in &branches {
            check(&history, *branch, expected);
        }
        for (imported_record, imported_value) in &imported {
            assert_eq!(
                *history.read(*imported_record).unwrap().value,
                *imported_value
            );
        }
    }
    let detached = history
        .materialize_branch(projection.head().branch())
        .unwrap();
    assert_eq!(detached.branch_count(), 1);
    assert_eq!(detached.last_branch(), history.last_branch());
    check(&detached, projection.head().branch(), &selected);
}
