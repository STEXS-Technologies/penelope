//! Canonical snapshots, explicit imports and corrected-away ancestor boundaries.
#![allow(clippy::indexing_slicing, clippy::unwrap_used)]
#[path = "support/snapshot_lookup.rs"]
mod snapshots;
use proptest::prelude::*;
proptest! {
    #[test]
    fn nested_snapshot_selection_matches_independent_oracle(data in prop::collection::vec(any::<u8>(), 0..64)) {
        snapshots::run(&data);
    }
}
#[test]
fn child_local_prefix_snapshots_and_exact_imports_keep_precedence() {
    for local in 0..=8 {
        let mut data = [255; 64];
        data[0] = 7;
        data[1] = 2;
        data[20] = local;
        snapshots::run(&data);
    }
}

#[test]
fn snapshot_integrity_is_rechecked_and_corrupt_exact_imports_do_not_fall_back() {
    use penelope_core::task::Digest;
    use penelope_timeline::*;
    use std::{cell::Cell, rc::Rc};
    #[derive(Clone)]
    struct MutableState {
        value: Rc<Cell<u64>>,
        bytes: Rc<Cell<u64>>,
    }
    impl HistoryPayload for MutableState {
        fn digest(&self) -> Digest {
            self.value.get().digest()
        }
        fn byte_len(&self) -> u64 {
            self.bytes.get()
        }
    }
    fn payload(value: u64) -> MutableState {
        MutableState {
            value: Rc::new(Cell::new(value)),
            bytes: Rc::new(Cell::new(8)),
        }
    }
    let scope = TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    };
    let root = TimelineBranchId::ZERO;
    let genesis = StateAnchor {
        scope,
        branch: root,
        step: TimelineStep::ZERO,
        snapshot: TimelineSnapshotId::from_digest(Digest([5; 32])),
        digest: 0_u64.digest(),
    };
    let mut history = FixedHistory::<MutableState, u64, 4, 8, 2>::new(
        genesis,
        payload(0),
        HistoryLimits {
            max_input_bytes: 8,
            max_snapshot_bytes: 16,
            max_total_bytes: 256,
        },
    )
    .unwrap();
    let record = InputRecord {
        scope,
        branch: root,
        step: TimelineStep::new(1),
        identity: TimelineInputId::from_digest(Digest([6; 32])),
        revision: InputRevision::ZERO,
        digest: 1_u64.digest(),
    };
    history.append_input(record, 1).unwrap();
    let canonical = StateAnchor {
        step: record.step,
        snapshot: TimelineSnapshotId::from_digest(Digest([7; 32])),
        digest: 1_u64.digest(),
        ..genesis
    };
    let original = payload(1);
    history
        .insert_snapshot(canonical, original.clone())
        .unwrap();
    let mut scratch = payload(0);
    original.bytes.set(0);
    assert_eq!(
        history.snapshot_at(root, record.step),
        Err(TimelineError::InputMismatch)
    );
    assert_eq!(
        history.restore(canonical, &mut scratch),
        Err(TimelineError::InputMismatch)
    );
    original.bytes.set(8);
    original.value.set(2);
    assert_eq!(
        history.snapshot_at(root, record.step),
        Err(TimelineError::InputMismatch)
    );
    assert_eq!(
        history.restore(canonical, &mut scratch),
        Err(TimelineError::InputMismatch)
    );
    original.value.set(1);
    let current =
        TimelineHead::new(scope, root, record.step, TimelineRevision::ZERO, None).unwrap();
    let plan = ReplayPlan::prepare(
        current,
        current.revision(),
        Correction::State(canonical),
        &[],
        ReplayLimits {
            max_retained_inputs: 4,
            max_replay_steps: 4,
        },
    )
    .unwrap();
    let mut projection = TimelineProjection::<1>::new(current);
    let reserved = projection
        .reserve(
            &plan,
            AuthorityObservation {
                scope,
                authority: TimelineAuthorityId::from_digest(Digest([8; 32])),
                epoch: AuthorityEpoch::ZERO,
                sequence: AuthoritySequence::new(1),
                step: canonical.step,
                digest: canonical.digest,
            },
        )
        .unwrap();
    history.fork(&plan, reserved).unwrap();
    let alias = StateAnchor {
        branch: reserved.branch(),
        ..canonical
    };
    let imported = payload(1);
    history.import_anchor(alias, imported.clone()).unwrap();
    history.restore(alias, &mut scratch).unwrap();
    imported.bytes.set(0);
    assert_eq!(
        history.snapshot_at(reserved.branch(), record.step).unwrap(),
        alias
    );
    assert_eq!(
        history.restore(alias, &mut scratch),
        Err(TimelineError::InputMismatch)
    );
    imported.bytes.set(8);
    imported.value.set(99);
    assert_eq!(
        history.snapshot_at(reserved.branch(), record.step).unwrap(),
        alias
    );
    assert_eq!(
        history.restore(alias, &mut scratch),
        Err(TimelineError::InputMismatch)
    );
}
