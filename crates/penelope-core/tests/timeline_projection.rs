//! Branch identity, authority ordering and revision-race model tests.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
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
fn head() -> TimelineHead {
    TimelineHead::new(
        scope(),
        TimelineBranchId::new(7),
        TimelineStep::new(4),
        TimelineRevision::new(9),
        None,
    )
    .unwrap()
}
const fn anchor() -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch: TimelineBranchId::new(7),
        step: TimelineStep::new(1),
        snapshot: TimelineSnapshotId::from_digest(Digest([8; 32])),
        digest: Digest([9; 32]),
    }
}
fn records() -> Vec<InputRecord> {
    (2..=4)
        .map(|step| InputRecord {
            scope: scope(),
            branch: TimelineBranchId::new(7),
            step: TimelineStep::new(step),
            identity: TimelineInputId::from_digest(Digest::of(&step.to_le_bytes())),
            revision: InputRevision::ZERO,
            digest: Digest::of(&step.to_le_bytes()),
        })
        .collect()
}
fn plan(records: &[InputRecord]) -> ReplayPlan<'_> {
    ReplayPlan::prepare(
        head(),
        head().revision(),
        Correction::State(anchor()),
        records,
        ReplayLimits {
            max_retained_inputs: 8,
            max_replay_steps: 8,
        },
    )
    .unwrap()
}
const fn observation(sequence: u64) -> AuthorityObservation {
    AuthorityObservation {
        scope: scope(),
        authority: TimelineAuthorityId::from_digest(Digest([10; 32])),
        epoch: AuthorityEpoch::ZERO,
        sequence: AuthoritySequence::new(sequence),
        step: anchor().step,
        digest: anchor().digest,
    }
}

#[test]
fn abandoned_branches_are_never_reused_even_after_restore() {
    let records = records();
    let plan = plan(&records);
    let mut projection = TimelineProjection::<2>::new(head());
    let first = projection.reserve(&plan, observation(1)).unwrap();
    let second = projection.reserve(&plan, observation(2)).unwrap();
    assert!(second.branch() > first.branch());
    assert_eq!(projection.head(), head());
    assert_eq!(
        projection.reserve(&plan, observation(3)),
        Err(TimelineError::Capacity)
    );
    projection.abort(first).unwrap();
    assert_eq!(
        projection.promote(first),
        Err(TimelineError::SessionUnavailable)
    );
    assert_eq!(
        projection.abort(first),
        Err(TimelineError::SessionUnavailable)
    );
    let third = projection.reserve(&plan, observation(3)).unwrap();
    assert!(third.branch() > second.branch());
    let high = projection.last_branch();
    let mut restored = TimelineProjection::<2>::restore(projection.head(), high).unwrap();
    assert_eq!(
        restored.promote(third),
        Err(TimelineError::SessionUnavailable)
    );
    assert!(restored.reserve(&plan, observation(4)).unwrap().branch() > high);
    assert!(TimelineProjection::<1>::restore(head(), TimelineBranchId::new(6)).is_err());
}
#[test]
fn exhaustive_competing_candidates_choose_exactly_one_revision() {
    for winner in 0..3 {
        let records = records();
        let plan = plan(&records);
        let mut projection = TimelineProjection::<3>::new(head());
        let reservations = [
            projection.reserve(&plan, observation(1)).unwrap(),
            projection.reserve(&plan, observation(2)).unwrap(),
            projection.reserve(&plan, observation(3)).unwrap(),
        ];
        let promoted = projection.promote(reservations[winner]).unwrap();
        assert_eq!(promoted.revision(), TimelineRevision::new(10));
        assert_eq!(promoted.branch(), reservations[winner].branch());
        assert_eq!(promoted.authority(), Some(observation((winner + 1) as u64)));
        assert_eq!(promoted.effect_finalized(), None);
        for (index, reservation) in reservations.into_iter().enumerate() {
            if index == winner {
                assert_eq!(
                    projection.promote(reservation),
                    Err(TimelineError::SessionUnavailable)
                );
            } else {
                assert_eq!(
                    projection.promote(reservation),
                    Err(TimelineError::Conflict)
                );
                projection.abort(reservation).unwrap();
            }
        }
        assert_eq!(projection.active_sessions(), 0);
    }
}
#[test]
fn new_live_inputs_observations_and_finality_supersede_pinned_replays() {
    for operation in 0..3 {
        let records = records();
        let plan = plan(&records);
        let mut projection = TimelineProjection::<1>::new(head());
        let reservation = projection.reserve(&plan, observation(1)).unwrap();
        match operation {
            0 => {
                let mut input = records[0];
                input.step = TimelineStep::new(5);
                projection.append(head(), input).unwrap();
            }
            1 => {
                projection.observe(head(), observation(2)).unwrap();
            }
            _ => {
                projection
                    .finalize_effects(head(), TimelineStep::new(1))
                    .unwrap();
            }
        }
        assert_eq!(
            projection.promote(reservation),
            Err(TimelineError::Conflict)
        );
        projection.abort(reservation).unwrap();
    }
}
#[test]
fn authority_epochs_are_ordered_and_do_not_reverse_effect_finality() {
    let mut projection = TimelineProjection::<1>::new(head());
    let observed = projection.observe(head(), observation(9)).unwrap();
    assert_eq!(observed.effect_finalized(), None);
    assert_eq!(
        projection.observe(observed, observation(9)),
        Err(TimelineError::StaleAuthority)
    );
    assert_eq!(
        projection.observe(observed, observation(8)),
        Err(TimelineError::StaleAuthority)
    );
    let finalized = projection
        .finalize_effects(observed, TimelineStep::new(2))
        .unwrap();
    let mut next = observation(1);
    next.epoch = AuthorityEpoch::new(1);
    let new_head = projection.observe(finalized, next).unwrap();
    assert_eq!(new_head.effect_finalized(), Some(TimelineStep::new(2)));
    assert_eq!(
        projection.finalize_effects(new_head, TimelineStep::new(1)),
        Err(TimelineError::InvalidFrontier)
    );
    assert_eq!(
        projection.finalize_effects(new_head, TimelineStep::new(5)),
        Err(TimelineError::InvalidFrontier)
    );
    next.authority = TimelineAuthorityId::from_digest(Digest([11; 32]));
    next.epoch = AuthorityEpoch::new(2);
    assert_eq!(
        projection.observe(new_head, next),
        Err(TimelineError::StaleAuthority)
    );
}
#[test]
fn overflow_zero_capacity_and_bad_bindings_do_not_change_projection() {
    let records = records();
    let plan = plan(&records);
    let mut full = TimelineProjection::<0>::new(head());
    assert_eq!(
        full.reserve(&plan, observation(1)),
        Err(TimelineError::Capacity)
    );
    let mut exhausted =
        TimelineProjection::<1>::restore(head(), TimelineBranchId::new(u64::MAX)).unwrap();
    assert_eq!(
        exhausted.reserve(&plan, observation(1)),
        Err(TimelineError::CounterExhausted)
    );
    let mut normal = TimelineProjection::<1>::new(head());
    let mut wrong = observation(1);
    wrong.digest = Digest([99; 32]);
    assert_eq!(
        normal.reserve(&plan, wrong),
        Err(TimelineError::InputMismatch)
    );
    wrong = observation(1);
    wrong.step = TimelineStep::new(5);
    assert_eq!(
        normal.reserve(&plan, wrong),
        Err(TimelineError::FutureCorrection)
    );
    wrong = observation(1);
    wrong.scope.model = TimelineModelId::from_digest(Digest([99; 32]));
    assert_eq!(
        normal.reserve(&plan, wrong),
        Err(TimelineError::ScopeMismatch)
    );
    assert_eq!(normal.head(), head());
    assert_eq!(normal.last_branch(), head().branch());
    assert_eq!(normal.active_sessions(), 0);
}

#[test]
fn accepted_observation_can_drive_its_own_correction_without_claiming_finality() {
    let records = records();
    let mut projection = TimelineProjection::<1>::new(head());
    let accepted = projection.observe(head(), observation(1)).unwrap();
    let plan = ReplayPlan::prepare(
        accepted,
        accepted.revision(),
        Correction::State(anchor()),
        &records,
        ReplayLimits {
            max_retained_inputs: 8,
            max_replay_steps: 8,
        },
    )
    .unwrap();
    let reservation = projection.reserve(&plan, observation(1)).unwrap();
    let promoted = projection.promote(reservation).unwrap();
    assert_eq!(promoted.authority(), Some(observation(1)));
    assert_eq!(promoted.effect_finalized(), None);
    assert_eq!(promoted.revision(), TimelineRevision::new(11));
}

#[test]
fn proposed_head_is_validation_metadata_and_never_a_publication_receipt() {
    let records = records();
    let expected = TimelineHead::new(
        scope(),
        head().branch(),
        head().step(),
        head().revision(),
        Some(TimelineStep::ZERO),
    )
    .unwrap();
    let plan = ReplayPlan::prepare(
        expected,
        expected.revision(),
        Correction::State(anchor()),
        &records,
        ReplayLimits {
            max_retained_inputs: 8,
            max_replay_steps: 8,
        },
    )
    .unwrap();
    let mut projection = TimelineProjection::<1>::new(expected);
    let reservation = projection.reserve(&plan, observation(1)).unwrap();
    let proposed = reservation.proposed_head().unwrap();
    assert_eq!(proposed.scope(), expected.scope());
    assert_eq!(proposed.branch(), reservation.branch());
    assert_eq!(proposed.step(), expected.step());
    assert_eq!(
        proposed.revision(),
        expected.revision().checked_next().unwrap()
    );
    assert_eq!(proposed.effect_finalized(), expected.effect_finalized());
    assert_eq!(proposed.authority(), Some(observation(1)));
    assert_eq!(projection.head(), expected);
    assert_eq!(projection.promotion_head(reservation).unwrap(), proposed);
    projection.abort(reservation).unwrap();
    // A copied descriptor cannot resurrect an aborted reservation.
    assert_eq!(reservation.proposed_head().unwrap(), proposed);
    assert_eq!(
        projection.promote(reservation),
        Err(TimelineError::SessionUnavailable)
    );
    let replacement = projection.reserve(&plan, observation(1)).unwrap();
    assert_eq!(
        projection.promote(replacement).unwrap(),
        replacement.proposed_head().unwrap()
    );
}

#[test]
fn proposed_head_reaches_the_maximum_revision_without_wrapping() {
    let records = records();
    let expected = TimelineHead::new(
        scope(),
        head().branch(),
        head().step(),
        TimelineRevision::new(u64::MAX - 1),
        None,
    )
    .unwrap();
    let limits = ReplayLimits {
        max_retained_inputs: 8,
        max_replay_steps: 8,
    };
    let plan = ReplayPlan::prepare(
        expected,
        expected.revision(),
        Correction::State(anchor()),
        &records,
        limits,
    )
    .unwrap();
    let mut projection = TimelineProjection::<1>::new(expected);
    let reservation = projection.reserve(&plan, observation(1)).unwrap();
    let proposed = reservation.proposed_head().unwrap();
    assert_eq!(proposed.revision().get(), u64::MAX);
    assert_eq!(projection.promote(reservation).unwrap(), proposed);
    let rebound: Vec<_> = records
        .iter()
        .map(|record| InputRecord {
            branch: proposed.branch(),
            ..*record
        })
        .collect();
    let correction = Correction::State(StateAnchor {
        branch: proposed.branch(),
        ..anchor()
    });
    assert_eq!(
        ReplayPlan::prepare(proposed, proposed.revision(), correction, &rebound, limits).err(),
        Some(TimelineError::CounterExhausted)
    );
    assert_eq!(projection.head(), proposed);
}
