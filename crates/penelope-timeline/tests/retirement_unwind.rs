//! A caught consumer destructor panic must not strand retired branch capacity.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

struct Payload {
    value: u64,
    panic_on: Rc<Cell<Option<u64>>>,
}
impl HistoryPayload for Payload {
    fn digest(&self) -> Digest {
        self.value.digest()
    }
    fn byte_len(&self) -> u64 {
        8
    }
}
impl Drop for Payload {
    fn drop(&mut self) {
        if self.panic_on.get() == Some(self.value) {
            self.panic_on.set(None);
            panic!("injected retired payload destructor panic");
        }
    }
}
fn anchor(scope: TimelineScope, branch: TimelineBranchId, step: u64, value: u64) -> StateAnchor {
    StateAnchor {
        scope,
        branch,
        step: TimelineStep::new(step),
        snapshot: TimelineSnapshotId::from_digest(value.digest()),
        digest: value.digest(),
    }
}
fn input(scope: TimelineScope, branch: TimelineBranchId, step: u64) -> InputRecord {
    InputRecord {
        scope,
        branch,
        step: TimelineStep::new(step),
        identity: TimelineInputId::from_digest(step.digest()),
        revision: InputRevision::ZERO,
        digest: step.digest(),
    }
}
#[test]
fn branch_retirement_can_resume_after_each_input_and_snapshot_destructor_panics() {
    let scope = TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    };
    let root = TimelineBranchId::ZERO;
    let panic_on = Rc::new(Cell::new(None));
    let payload = |value| Payload {
        value,
        panic_on: Rc::clone(&panic_on),
    };
    let mut history = FixedHistory::<Payload, Payload, 8, 8, 3>::new(
        anchor(scope, root, 0, 0),
        payload(0),
        HistoryLimits {
            max_input_bytes: 8,
            max_snapshot_bytes: 8,
            max_total_bytes: 128,
        },
    )
    .unwrap();
    for step in 1..=3 {
        history
            .append_input(input(scope, root, step), payload(step))
            .unwrap();
    }
    let live = TimelineHead::new(
        scope,
        root,
        TimelineStep::new(3),
        TimelineRevision::new(3),
        None,
    )
    .unwrap();
    let corrected = anchor(scope, root, 1, 100);
    history.import_anchor(corrected, payload(100)).unwrap();
    let records = [input(scope, root, 2), input(scope, root, 3)];
    let plan = ReplayPlan::prepare(
        live,
        live.revision(),
        Correction::State(corrected),
        &records,
        ReplayLimits {
            max_retained_inputs: 2,
            max_replay_steps: 2,
        },
    )
    .unwrap();
    let mut projection = TimelineProjection::<1>::new(live);
    let reservation = projection
        .reserve(
            &plan,
            AuthorityObservation {
                scope,
                authority: TimelineAuthorityId::from_digest(Digest([5; 32])),
                epoch: AuthorityEpoch::ZERO,
                sequence: AuthoritySequence::ZERO,
                step: corrected.step,
                digest: corrected.digest,
            },
        )
        .unwrap();
    let child = reservation.branch();
    history.fork(&plan, reservation).unwrap();
    projection.abort(reservation).unwrap();
    for step in 4..=5 {
        history
            .append_input(input(scope, child, step), payload(step))
            .unwrap();
        history
            .insert_snapshot(anchor(scope, child, step, step * 10), payload(step * 10))
            .unwrap();
    }
    // Fail once on each owned payload, then retry the same retirement. Parent
    // records and the branch high-water mark must survive every partial cleanup.
    for failing_value in [4, 5, 40, 50] {
        panic_on.set(Some(failing_value));
        let failed = catch_unwind(AssertUnwindSafe(|| history.retire_branch(child, live)));
        assert!(failed.is_err());
        assert_eq!(panic_on.get(), None);
        assert_eq!(history.branch_count(), 2);
        assert_eq!(history.last_branch(), child);
        assert_eq!(
            history.payload_bytes(),
            u64::try_from(history.input_count() + history.anchor_count()).unwrap() * 8
        );
        assert_eq!(
            history.input_at(root, TimelineStep::new(3)).unwrap(),
            records[1]
        );
        assert_eq!(
            history.snapshot_at(root, TimelineStep::ZERO).unwrap(),
            anchor(scope, root, 0, 0)
        );
    }
    history.retire_branch(child, live).unwrap();
    assert_eq!(history.branch_count(), 1);
    assert_eq!(history.input_count(), 3);
    assert_eq!(history.anchor_count(), 2);
    assert_eq!(history.payload_bytes(), 40);
    assert_eq!(history.last_branch(), child);
    let next = projection
        .reserve(&plan, reservation.observation())
        .unwrap();
    assert!(next.branch() > child);
    history.fork(&plan, next).unwrap();
}
