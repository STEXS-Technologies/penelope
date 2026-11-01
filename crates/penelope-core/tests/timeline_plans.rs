//! Reference model and adversarial planning checks; no application state is published.
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
fn head(step: u64, finalized: Option<u64>) -> TimelineHead {
    TimelineHead::new(
        scope(),
        TimelineBranchId::new(7),
        TimelineStep::new(step),
        TimelineRevision::new(9),
        finalized.map(TimelineStep::new),
    )
    .unwrap()
}
const fn anchor(step: u64) -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch: TimelineBranchId::new(7),
        step: TimelineStep::new(step),
        snapshot: TimelineSnapshotId::from_digest(Digest([5; 32])),
        digest: Digest([6; 32]),
    }
}
fn inputs(first: u64, last: u64) -> Vec<InputRecord> {
    (first..=last)
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
const LIMITS: ReplayLimits = ReplayLimits {
    max_retained_inputs: 16,
    max_replay_steps: 16,
};
fn plan(
    h: TimelineHead,
    c: Correction,
    records: &[InputRecord],
) -> Result<ReplayPlan<'_>, TimelineError> {
    ReplayPlan::prepare(h, h.revision(), c, records, LIMITS)
}
fn error(h: TimelineHead, c: Correction, records: &[InputRecord], expected: TimelineError) {
    assert_eq!(plan(h, c, records).unwrap_err(), expected);
}

#[test]
fn exhaustive_short_state_corrections_match_integer_reference() {
    for end in 0..=12 {
        for corrected in 0..=end {
            let records = inputs(corrected + 1, end);
            let p = plan(
                head(end, None),
                Correction::State(anchor(corrected)),
                &records,
            )
            .unwrap();
            assert_eq!(p.len(), (end - corrected) as usize);
            let mut scratch = 100_u64; // trusted corrected post-step state
            for offset in 0..p.len() {
                scratch += p.input(offset).unwrap().step.get();
            }
            assert_eq!(scratch, 100 + ((corrected + 1)..=end).sum::<u64>());
            assert_eq!(p.input(p.len()), None);
            assert_eq!(p.is_empty(), corrected == end);
        }
    }
}
#[test]
fn exhaustive_input_corrections_restore_exact_predecessor_and_replace_once() {
    for end in 1..=12 {
        for corrected in 1..=end {
            let records = inputs(corrected, end);
            let mut replacement = records[0];
            replacement.revision = InputRevision::new(1);
            replacement.digest = Digest([99; 32]);
            let p = plan(
                head(end, None),
                Correction::Input {
                    anchor: anchor(corrected - 1),
                    replacement,
                },
                &records,
            )
            .unwrap();
            let mut scratch = (1..corrected).sum::<u64>();
            for offset in 0..p.len() {
                let record = p.input(offset).unwrap();
                scratch += if record.digest == replacement.digest {
                    100
                } else {
                    record.step.get()
                };
            }
            assert_eq!(
                scratch,
                (1..corrected).sum::<u64>() + 100 + ((corrected + 1)..=end).sum::<u64>()
            );
            assert_eq!(records[0].revision, InputRevision::ZERO);
        }
    }
}
#[test]
fn rejects_gaps_reordering_missing_and_extra_inputs() {
    let h = head(4, None);
    let c = Correction::State(anchor(1));
    let original = inputs(2, 4);
    for records in [
        inputs(3, 4),
        inputs(2, 3),
        inputs(2, 5),
        vec![original[1], original[0], original[2]],
        vec![original[0], original[0], original[2]],
    ] {
        error(h, c, &records, TimelineError::HistoryUnavailable);
    }
}
#[test]
fn validates_every_scope_and_branch_without_mutating_head() {
    let h = head(4, None);
    let original = inputs(2, 4);
    for index in 0..original.len() {
        for role in 0..4 {
            let mut records = original.clone();
            let scope = &mut records[index].scope;
            match role {
                0 => scope.tenant = TimelineTenantId::from_digest(Digest([9; 32])),
                1 => scope.timeline = TimelineId::from_digest(Digest([9; 32])),
                2 => scope.model = TimelineModelId::from_digest(Digest([9; 32])),
                _ => scope.schema = TimelineSchemaId::from_digest(Digest([9; 32])),
            }
            error(
                h,
                Correction::State(anchor(1)),
                &records,
                TimelineError::ScopeMismatch,
            );
        }
        let mut records = original.clone();
        records[index].branch = TimelineBranchId::new(8);
        error(
            h,
            Correction::State(anchor(1)),
            &records,
            TimelineError::Conflict,
        );
    }
    let mut wrong = anchor(1);
    wrong.scope.schema = TimelineSchemaId::from_digest(Digest([8; 32]));
    error(
        h,
        Correction::State(wrong),
        &original,
        TimelineError::ScopeMismatch,
    );
    wrong = anchor(1);
    wrong.branch = TimelineBranchId::new(8);
    error(
        h,
        Correction::State(wrong),
        &original,
        TimelineError::Conflict,
    );
    assert_eq!(h, head(4, None));
}
#[test]
fn effect_finality_is_separate_and_cannot_be_corrected_away() {
    error(
        head(4, Some(2)),
        Correction::State(anchor(2)),
        &inputs(3, 4),
        TimelineError::ReconciliationRequired,
    );
    error(
        head(4, Some(2)),
        Correction::State(anchor(1)),
        &inputs(2, 4),
        TimelineError::ReconciliationRequired,
    );
    let records = inputs(2, 4);
    let mut replacement = records[0];
    replacement.revision = InputRevision::new(1);
    error(
        head(4, Some(2)),
        Correction::Input {
            anchor: anchor(1),
            replacement,
        },
        &records,
        TimelineError::ReconciliationRequired,
    );
    assert!(
        plan(
            head(4, Some(2)),
            Correction::State(anchor(3)),
            &inputs(4, 4)
        )
        .is_ok()
    );
    let later_records = inputs(3, 4);
    let mut later_replacement = later_records[0];
    later_replacement.revision = InputRevision::new(1);
    assert!(
        plan(
            head(4, Some(2)),
            Correction::Input {
                anchor: anchor(2),
                replacement: later_replacement
            },
            &later_records
        )
        .is_ok()
    );
    assert_eq!(
        TimelineHead::new(
            scope(),
            TimelineBranchId::ZERO,
            TimelineStep::new(1),
            TimelineRevision::ZERO,
            Some(TimelineStep::new(2))
        ),
        Err(TimelineError::InvalidFrontier)
    );
}
#[test]
fn replacement_requires_same_identity_newer_revision_and_exact_anchor() {
    let h = head(4, None);
    let records = inputs(2, 4);
    let mut replacement = records[0];
    error(
        h,
        Correction::Input {
            anchor: anchor(1),
            replacement,
        },
        &records,
        TimelineError::InputMismatch,
    );
    replacement.revision = InputRevision::new(1);
    let valid = replacement;
    replacement.identity = TimelineInputId::from_digest(Digest([44; 32]));
    error(
        h,
        Correction::Input {
            anchor: anchor(1),
            replacement,
        },
        &records,
        TimelineError::InputMismatch,
    );
    replacement = valid;
    replacement.step = TimelineStep::new(3);
    error(
        h,
        Correction::Input {
            anchor: anchor(1),
            replacement,
        },
        &records,
        TimelineError::HistoryUnavailable,
    );
    replacement = valid;
    replacement.scope.model = TimelineModelId::from_digest(Digest([44; 32]));
    error(
        h,
        Correction::Input {
            anchor: anchor(1),
            replacement,
        },
        &records,
        TimelineError::ScopeMismatch,
    );
    replacement = valid;
    replacement.branch = TimelineBranchId::new(8);
    error(
        h,
        Correction::Input {
            anchor: anchor(1),
            replacement,
        },
        &records,
        TimelineError::Conflict,
    );
}
#[test]
fn counters_limits_future_and_stale_publications_fail_closed() {
    let h = head(4, None);
    let records = inputs(2, 4);
    let c = Correction::State(anchor(1));
    for limits in [
        ReplayLimits {
            max_retained_inputs: 2,
            max_replay_steps: 16,
        },
        ReplayLimits {
            max_retained_inputs: 16,
            max_replay_steps: 2,
        },
    ] {
        assert_eq!(
            ReplayPlan::prepare(h, h.revision(), c, &records, limits).unwrap_err(),
            TimelineError::Capacity
        );
    }
    assert_eq!(
        ReplayPlan::prepare(h, TimelineRevision::new(8), c, &records, LIMITS).unwrap_err(),
        TimelineError::Conflict
    );
    error(
        h,
        Correction::State(anchor(5)),
        &[],
        TimelineError::FutureCorrection,
    );
    let exhausted = TimelineHead::new(
        scope(),
        h.branch(),
        h.step(),
        TimelineRevision::new(u64::MAX),
        None,
    )
    .unwrap();
    error(exhausted, c, &records, TimelineError::CounterExhausted);
    assert_eq!(TimelineStep::new(u64::MAX).checked_next(), None);
    assert_eq!(TimelineBranchId::new(u64::MAX).checked_next(), None);
    assert_eq!(InputRevision::new(u64::MAX).checked_next(), None);
    let p = plan(h, c, &records).unwrap();
    assert!(p.check_head(h).is_ok());
    for changed in [
        head(5, None),
        head(4, Some(1)),
        TimelineHead::new(
            scope(),
            TimelineBranchId::new(8),
            h.step(),
            h.revision(),
            None,
        )
        .unwrap(),
        TimelineHead::new(
            scope(),
            h.branch(),
            h.step(),
            TimelineRevision::new(10),
            None,
        )
        .unwrap(),
    ] {
        assert_eq!(p.check_head(changed), Err(TimelineError::Conflict));
    }
}
