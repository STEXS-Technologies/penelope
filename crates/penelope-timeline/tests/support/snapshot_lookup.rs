//! Snapshot-selection oracle independent of physical slots and ancestry traversal.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::collections::BTreeMap;
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
fn anchor(branch: TimelineBranchId, step: usize, value: u64) -> StateAnchor {
    let mut key = [0; 16];
    key[..8].copy_from_slice(&(step as u64).to_le_bytes());
    key[8..].copy_from_slice(&value.to_le_bytes());
    StateAnchor {
        scope: scope(),
        branch,
        step: TimelineStep::new(step as u64),
        snapshot: TimelineSnapshotId::from_digest(Digest::of(&key)),
        digest: value.digest(),
    }
}
fn input(step: usize) -> InputRecord {
    InputRecord {
        scope: scope(),
        branch: TimelineBranchId::ZERO,
        step: TimelineStep::new(step as u64),
        identity: TimelineInputId::from_digest((step as u64).digest()),
        revision: InputRevision::ZERO,
        digest: (step as u64).digest(),
    }
}
type History = FixedHistory<u64, u64, 32, 64, 4>;
type Oracle = BTreeMap<usize, (StateAnchor, u64)>;
fn check(history: &History, branch: TimelineBranchId, count: usize, expected: &Oracle) {
    for step in 0..=count {
        let found = history.snapshot_at(branch, TimelineStep::new(step as u64));
        if let Some((wanted, value)) = expected.get(&step) {
            assert_eq!(found.unwrap(), *wanted);
            let mut scratch = u64::MAX;
            assert_eq!(history.restore(*wanted, &mut scratch).unwrap(), *wanted);
            assert_eq!(scratch, *value);
            let forged = StateAnchor {
                snapshot: TimelineSnapshotId::from_digest(Digest([255; 32])),
                ..*wanted
            };
            assert!(matches!(
                history.restore(forged, &mut scratch),
                Err(TimelineError::InputMismatch)
            ));
            let foreign = StateAnchor {
                scope: TimelineScope {
                    schema: TimelineSchemaId::from_digest(Digest([254; 32])),
                    ..wanted.scope
                },
                ..*wanted
            };
            assert!(matches!(
                history.restore(foreign, &mut scratch),
                Err(TimelineError::ScopeMismatch)
            ));
        } else {
            assert_eq!(found, Err(TimelineError::HistoryUnavailable));
        }
    }
}
pub fn run(data: &[u8]) {
    let count = usize::from(byte(data, 0) % 8) + 1;
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
    let mut selected = Oracle::from([(0, (anchor(root, 0, 0), 0))]);
    for step in 1..=count {
        history.append_input(input(step), step as u64).unwrap();
        let value = step as u64 * 10;
        let canonical = anchor(root, step, value);
        history.insert_snapshot(canonical, value).unwrap();
        selected.insert(step, (canonical, value));
    }
    let mut projection = TimelineProjection::<1>::new(
        TimelineHead::new(
            scope(),
            root,
            TimelineStep::new(count as u64),
            TimelineRevision::ZERO,
            None,
        )
        .unwrap(),
    );
    let mut branches = vec![(root, selected.clone())];
    let mut imports = Vec::new();
    for generation in 0..usize::from(byte(data, 1) % 3) + 1 {
        let parent = projection.head();
        let cut = usize::from(byte(data, generation + 2)) % (count + 1);
        let value = u64::from(byte(data, generation + 10)) + 10000 + generation as u64;
        let corrected = anchor(parent.branch(), cut, value);
        history.import_anchor(corrected, value).unwrap();
        imports.push((corrected, value));
        let mut scratch = [input(0); 32];
        let records = history
            .collect_inputs(parent.branch(), corrected.step, parent.step(), &mut scratch)
            .unwrap();
        let plan = ReplayPlan::prepare(
            parent,
            parent.revision(),
            Correction::State(corrected),
            records,
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
            step: corrected.step,
            digest: corrected.digest,
        };
        let reservation = projection.reserve(&plan, observed).unwrap();
        history.fork(&plan, reservation).unwrap();
        let child = projection.promote(reservation).unwrap();
        selected.retain(|step, _| *step < cut);
        for (stored, _) in selected.values_mut() {
            stored.branch = child.branch();
        }
        selected.insert(
            cut,
            (
                StateAnchor {
                    branch: child.branch(),
                    ..corrected
                },
                value,
            ),
        );
        // Canonical child snapshots may occur inside the inherited input prefix,
        // on either side of the state correction, and must beat ancestor/import data.
        let local_step = usize::from(byte(data, generation + 20)) % (count + 1);
        let local_value = u64::from(byte(data, generation + 30)) + 20000;
        let local = anchor(child.branch(), local_step, local_value);
        history.insert_snapshot(local, local_value).unwrap();
        selected.insert(local_step, (local, local_value));
        let imported_value = local_value + 10000;
        let imported = anchor(child.branch(), local_step, imported_value);
        history.import_anchor(imported, imported_value).unwrap();
        imports.push((imported, imported_value));
        branches.push((child.branch(), selected.clone()));
        for (branch, expected) in &branches {
            check(&history, *branch, count, expected);
        }
        for (imported_anchor, imported_payload) in &imports {
            let mut restored = 0;
            history.restore(*imported_anchor, &mut restored).unwrap();
            assert_eq!(restored, *imported_payload);
        }
    }
    let detached = history
        .materialize_branch(projection.head().branch())
        .unwrap();
    assert_eq!(detached.anchor_count(), selected.len());
    check(&detached, projection.head().branch(), count, &selected);
}
