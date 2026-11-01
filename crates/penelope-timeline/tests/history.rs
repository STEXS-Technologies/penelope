//! Full-snapshot reference histories, immutable ancestry, capacity and eviction checks.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::{cell::Cell, rc::Rc};

const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
fn anchor(branch: u64, step: u64, state: u64, nonce: u8) -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch: TimelineBranchId::new(branch),
        step: TimelineStep::new(step),
        snapshot: TimelineSnapshotId::from_digest(Digest::of(&[nonce])),
        digest: HistoryPayload::digest(&state),
    }
}
fn input(branch: u64, step: u64, value: u64) -> InputRecord {
    InputRecord {
        scope: scope(),
        branch: TimelineBranchId::new(branch),
        step: TimelineStep::new(step),
        identity: TimelineInputId::from_digest(Digest::of(&step.to_le_bytes())),
        revision: InputRevision::ZERO,
        digest: HistoryPayload::digest(&value),
    }
}
const LIMITS: HistoryLimits = HistoryLimits {
    max_input_bytes: 8,
    max_snapshot_bytes: 8,
    max_total_bytes: 1024,
};
const EFFECTS: EffectLimits = EffectLimits {
    max_intents: 0,
    max_payload_bytes: 0,
};
const REPLAY: ReplayLimits = ReplayLimits {
    max_retained_inputs: 16,
    max_replay_steps: 16,
};
type History = FixedHistory<u64, u64, 32, 32, 8>;
type Live = LiveTimeline<u64, (), 4, 4>;
fn history() -> History {
    let mut history = History::new(anchor(0, 0, 0, 0), 0, LIMITS).unwrap();
    let mut state = 0;
    for step in 1..=4 {
        history.append_input(input(0, step, step), step).unwrap();
        state += step;
        history
            .insert_snapshot(anchor(0, step, state, step as u8), state)
            .unwrap();
    }
    history
}
fn head() -> TimelineHead {
    TimelineHead::new(
        scope(),
        TimelineBranchId::ZERO,
        TimelineStep::new(4),
        TimelineRevision::new(4),
        None,
    )
    .unwrap()
}
const fn observation(
    head: TimelineHead,
    step: TimelineStep,
    digest: Digest,
    sequence: u64,
) -> AuthorityObservation {
    AuthorityObservation {
        scope: head.scope(),
        authority: TimelineAuthorityId::from_digest(Digest([9; 32])),
        epoch: AuthorityEpoch::ZERO,
        sequence: AuthoritySequence::new(sequence),
        step,
        digest,
    }
}
struct Model;
impl RewindableComputation for Model {
    type State = u64;
    type Input = u64;
    type Effect = ();
    type Error = ();
    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut u64,
        input: &u64,
        _context: StepContext,
        _effects: &mut S,
    ) -> Result<(), ()> {
        *state += *input;
        Ok(())
    }
}
struct Valid;
impl CandidateValidator<u64> for Valid {
    fn validate(&self, _state: &u64, _head: TimelineHead) -> Result<(), TimelineError> {
        Ok(())
    }
}

#[test]
fn state_then_input_correction_preserve_parent_facts_and_resolve_ancestry() {
    let mut history = history();
    let mut live = Live::new(head(), 10, EFFECTS);
    let corrected = anchor(0, 1, 100, 99);
    history.import_anchor(corrected, 100).unwrap();
    assert_eq!(
        history
            .snapshot_at(TimelineBranchId::ZERO, TimelineStep::new(1))
            .unwrap()
            .digest,
        HistoryPayload::digest(&1_u64)
    );
    let mut scratch = [input(0, 1, 1); 16];
    let records = history
        .collect_inputs(
            head().branch(),
            TimelineStep::new(1),
            head().step(),
            &mut scratch,
        )
        .unwrap();
    let plan = ReplayPlan::prepare(
        head(),
        head().revision(),
        Correction::State(corrected),
        records,
        REPLAY,
    )
    .unwrap();
    let reservation = live
        .reserve(
            &plan,
            observation(head(), corrected.step, corrected.digest, 1),
        )
        .unwrap();
    history.fork(&plan, reservation).unwrap();
    assert!(
        history
            .snapshot_at(reservation.branch(), TimelineStep::new(2))
            .is_err()
    ); // Parent state after the correction is invalid.
    let mut session = history
        .begin_replay::<(), 4>(plan, reservation, 0, EFFECTS)
        .unwrap();
    let mut snapshots = Vec::new();
    for step in 2..=4 {
        session.advance(head(), &Model, 1).unwrap();
        snapshots.push((step, *session.scratch()));
    }
    let candidate = session.validate(head(), &Valid).unwrap(); // Releases the immutable history pin.
    for (step, state) in snapshots {
        history
            .insert_snapshot(
                anchor(reservation.branch().get(), step, state, step as u8),
                state,
            )
            .unwrap();
    }
    let first = live.promote(candidate).unwrap();
    assert_eq!(live.read(), (first, &109));
    assert_eq!(
        *history
            .read(
                history
                    .input_at(first.branch(), TimelineStep::new(3))
                    .unwrap()
            )
            .unwrap()
            .value,
        3
    );
    let predecessor = history
        .snapshot_at(first.branch(), TimelineStep::new(2))
        .unwrap();
    assert_eq!(predecessor.digest, HistoryPayload::digest(&102_u64));
    let mut replacement = history
        .input_at(first.branch(), TimelineStep::new(3))
        .unwrap();
    replacement.revision = InputRevision::new(1);
    replacement.digest = HistoryPayload::digest(&30_u64);
    history.import_input(replacement, 30).unwrap();
    let mut second_scratch = [input(0, 1, 1); 16];
    let second_records = history
        .collect_inputs(
            first.branch(),
            TimelineStep::new(2),
            first.step(),
            &mut second_scratch,
        )
        .unwrap();
    let second_plan = ReplayPlan::prepare(
        first,
        first.revision(),
        Correction::Input {
            anchor: predecessor,
            replacement,
        },
        second_records,
        REPLAY,
    )
    .unwrap();
    let second_reservation = live
        .reserve(
            &second_plan,
            observation(first, replacement.step, replacement.digest, 2),
        )
        .unwrap();
    history.fork(&second_plan, second_reservation).unwrap();
    let mut second_session = history
        .begin_replay::<(), 4>(second_plan, second_reservation, 0, EFFECTS)
        .unwrap();
    second_session.advance(first, &Model, 8).unwrap();
    assert_eq!(*second_session.scratch(), 136);
    let second_candidate = second_session.validate(first, &Valid).unwrap();
    history
        .insert_snapshot(anchor(second_reservation.branch().get(), 4, 136, 88), 136)
        .unwrap();
    let second = live.promote(second_candidate).unwrap();
    assert_eq!(live.read(), (second, &136));
    let corrected_input = history
        .input_at(second.branch(), TimelineStep::new(3))
        .unwrap();
    assert_eq!(corrected_input.revision, InputRevision::new(1));
    assert_eq!(*history.read(corrected_input).unwrap().value, 30);
    assert_eq!(
        history
            .snapshot_at(head().branch(), head().step())
            .unwrap()
            .digest,
        HistoryPayload::digest(&10_u64)
    );
    assert_eq!(
        history
            .snapshot_at(first.branch(), first.step())
            .unwrap()
            .digest,
        HistoryPayload::digest(&109_u64)
    );
    assert_eq!(
        history
            .snapshot_at(second.branch(), TimelineStep::new(1))
            .unwrap()
            .digest,
        HistoryPayload::digest(&100_u64)
    );
    let detached = history.materialize_branch(second.branch()).unwrap();
    assert_eq!(detached.branch_count(), 1);
    assert_eq!(detached.input_count(), 4);
    assert!(detached.payload_bytes() < history.payload_bytes());
    assert_eq!(
        detached
            .input_at(second.branch(), TimelineStep::new(3))
            .unwrap(),
        corrected_input
    );
    assert_eq!(*detached.read(corrected_input).unwrap().value, 30);
    assert_eq!(
        detached
            .snapshot_at(second.branch(), TimelineStep::new(1))
            .unwrap()
            .digest,
        HistoryPayload::digest(&100_u64)
    );
    assert_eq!(
        detached
            .snapshot_at(second.branch(), TimelineStep::new(2))
            .unwrap()
            .digest,
        HistoryPayload::digest(&102_u64)
    );
    assert_eq!(
        detached.snapshot_at(second.branch(), TimelineStep::new(3)),
        Err(TimelineError::HistoryUnavailable)
    ); // Invalidated parent S3 stays absent.
    let mut restored = 0;
    detached
        .restore_current(
            second,
            detached
                .snapshot_at(second.branch(), second.step())
                .unwrap(),
            &mut restored,
        )
        .unwrap();
    assert_eq!(restored, 136);
    assert_eq!(
        detached.input_at(first.branch(), TimelineStep::new(3)),
        Err(TimelineError::HistoryUnavailable)
    );
    // Copying an older branch must not forget IDs consumed by newer candidates.
    let mut older = history.materialize_branch(first.branch()).unwrap();
    assert_eq!(older.last_branch(), history.last_branch());
    let earlier_anchor = older
        .snapshot_at(first.branch(), TimelineStep::new(1))
        .unwrap();
    let mut older_scratch = [input(0, 1, 1); 16];
    let older_records = older
        .collect_inputs(
            first.branch(),
            TimelineStep::new(1),
            first.step(),
            &mut older_scratch,
        )
        .unwrap();
    let older_plan = ReplayPlan::prepare(
        first,
        first.revision(),
        Correction::State(earlier_anchor),
        older_records,
        REPLAY,
    )
    .unwrap();
    let observed = observation(first, earlier_anchor.step, earlier_anchor.digest, 3);
    let mut restarted = TimelineProjection::<1>::new(first);
    let reused = restarted.reserve(&older_plan, observed).unwrap();
    assert_eq!(reused.branch(), second.branch());
    assert_eq!(
        older.fork(&older_plan, reused),
        Err(TimelineError::Conflict)
    );
    let mut preserved = TimelineProjection::<1>::restore(first, second.branch()).unwrap();
    let fresh = preserved.reserve(&older_plan, observed).unwrap();
    older.fork(&older_plan, fresh).unwrap();
    assert!(fresh.branch() > second.branch());
}
#[test]
fn unaccepted_correction_import_is_never_a_live_step_restore_anchor() {
    let mut history = history();
    let corrected = anchor(0, 4, 999, 99);
    history.import_anchor(corrected, 999).unwrap();
    let mut scratch = 0;
    assert_eq!(
        history.restore_current(head(), corrected, &mut scratch),
        Err(TimelineError::InputMismatch)
    );
    assert_eq!(scratch, 0);
    let canonical = history.snapshot_at(head().branch(), head().step()).unwrap();
    history
        .restore_current(head(), canonical, &mut scratch)
        .unwrap();
    assert_eq!(scratch, 10);
}
#[test]
fn prepared_history_rejects_unselected_imported_versions_in_other_replay_steps() {
    let mut history = history();
    let corrected = anchor(0, 1, 100, 99);
    history.import_anchor(corrected, 100).unwrap();
    let mut impostor = input(0, 3, 30);
    impostor.revision = InputRevision::new(1);
    history.import_input(impostor, 30).unwrap();
    let records = [input(0, 2, 2), impostor, input(0, 4, 4)];
    let plan = ReplayPlan::prepare(
        head(),
        head().revision(),
        Correction::State(corrected),
        &records,
        REPLAY,
    )
    .unwrap();
    let mut projection = TimelineProjection::<1>::new(head());
    let reservation = projection
        .reserve(
            &plan,
            observation(head(), corrected.step, corrected.digest, 1),
        )
        .unwrap();
    assert!(history.fork(&plan, reservation).is_err());
    assert!(
        history
            .begin_replay::<(), 4>(plan, reservation, 0, EFFECTS)
            .is_err()
    );
    assert_eq!(history.branch_count(), 1);
}
#[test]
fn eviction_preserves_complete_floor_anchor_and_refuses_out_of_window_replay() {
    let mut history = history();
    let before = history.payload_bytes();
    history.expire_before(TimelineStep::new(2)).unwrap();
    assert_eq!(history.floor(), TimelineStep::new(2));
    assert_eq!(history.input_count(), 2);
    assert_eq!(history.anchor_count(), 3);
    assert_eq!(history.payload_bytes(), before - 32);
    assert!(
        history
            .input_at(head().branch(), TimelineStep::new(2))
            .is_err()
    );
    assert!(
        history
            .snapshot_at(head().branch(), TimelineStep::new(1))
            .is_err()
    );
    let mut restored = 0;
    let floor = history
        .snapshot_at(head().branch(), TimelineStep::new(2))
        .unwrap();
    history.restore(floor, &mut restored).unwrap();
    assert_eq!(restored, 3);
    assert_eq!(
        history.expire_before(TimelineStep::new(1)),
        Err(TimelineError::HistoryUnavailable)
    );
    assert_eq!(
        history.expire_before(TimelineStep::new(5)),
        Err(TimelineError::HistoryUnavailable)
    );
    history.append_input(input(0, 5, 5), 5).unwrap();
    assert_eq!(history.input_count(), 3);
}
#[test]
fn missing_floor_anchor_cannot_be_evicted_to_make_space() {
    let mut history = History::new(anchor(0, 0, 0, 0), 0, LIMITS).unwrap();
    for step in 1..=4 {
        history.append_input(input(0, step, step), step).unwrap();
    }
    let before = history.payload_bytes();
    assert_eq!(
        history.expire_before(TimelineStep::new(2)),
        Err(TimelineError::HistoryUnavailable)
    );
    assert_eq!(history.floor(), TimelineStep::ZERO);
    assert_eq!(history.payload_bytes(), before);
    assert_eq!(history.input_count(), 4);
}
#[test]
fn duplicate_changed_scope_and_quota_admission_leave_existing_data_unchanged() {
    let mut history = history();
    let before = history.payload_bytes();
    history.append_input(input(0, 1, 1), 1).unwrap();
    assert_eq!(history.payload_bytes(), before);
    let mut changed = input(0, 1, 99);
    assert!(history.append_input(changed, 99).is_err());
    assert!(history.import_input(changed, 99).is_err());
    changed = input(0, 5, 5);
    changed.scope.model = TimelineModelId::from_digest(Digest([99; 32]));
    assert_eq!(
        history.append_input(changed, 5),
        Err(TimelineError::ScopeMismatch)
    );
    assert_eq!(history.payload_bytes(), before);
    let mut small = FixedHistory::<u64, u64, 1, 1, 1>::new(
        anchor(0, 0, 0, 0),
        0,
        HistoryLimits {
            max_total_bytes: 16,
            ..LIMITS
        },
    )
    .unwrap();
    small.append_input(input(0, 1, 1), 1).unwrap();
    assert_eq!(
        small.append_input(input(0, 2, 2), 2),
        Err(TimelineError::Capacity)
    );
    assert_eq!(small.input_count(), 1);
    assert_eq!(small.payload_bytes(), 16);
    assert!(FixedHistory::<u64, u64, 1, 0, 1>::new(anchor(0, 0, 0, 0), 0, LIMITS).is_err());
    assert!(FixedHistory::<u64, u64, 1, 1, 0>::new(anchor(0, 0, 0, 0), 0, LIMITS).is_err());
    assert!(
        FixedHistory::<u64, u64, 1, 1, 1>::new(
            anchor(0, 0, 0, 0),
            0,
            HistoryLimits {
                max_snapshot_bytes: 7,
                ..LIMITS
            }
        )
        .is_err()
    );
}
#[test]
fn dependent_or_live_branches_cannot_be_retired_and_retired_ids_stay_consumed() {
    let mut history = history();
    let corrected = anchor(0, 1, 100, 99);
    history.import_anchor(corrected, 100).unwrap();
    let records = [input(0, 2, 2), input(0, 3, 3), input(0, 4, 4)];
    let plan = ReplayPlan::prepare(
        head(),
        head().revision(),
        Correction::State(corrected),
        &records,
        REPLAY,
    )
    .unwrap();
    let mut projection = TimelineProjection::<1>::new(head());
    let reservation = projection
        .reserve(
            &plan,
            observation(head(), corrected.step, corrected.digest, 1),
        )
        .unwrap();
    history.fork(&plan, reservation).unwrap();
    assert_eq!(
        history.retire_branch(head().branch(), head()),
        Err(TimelineError::Conflict)
    );
    let prospective = projection.promotion_head(reservation).unwrap();
    assert_eq!(
        history.retire_branch(head().branch(), prospective),
        Err(TimelineError::Conflict)
    );
    assert_eq!(
        history.retire_branch(reservation.branch(), prospective),
        Err(TimelineError::Conflict)
    );
    history.retire_branch(reservation.branch(), head()).unwrap();
    assert_eq!(history.branch_count(), 1);
    assert_eq!(
        history.fork(&plan, reservation),
        Err(TimelineError::Conflict)
    );
}
struct MutablePayload {
    value: Rc<Cell<u64>>,
}
impl HistoryPayload for MutablePayload {
    fn digest(&self) -> Digest {
        HistoryPayload::digest(&self.value.get())
    }
    fn byte_len(&self) -> u64 {
        8
    }
}
#[test]
fn read_boundaries_detect_consumer_interior_mutation_instead_of_caching_integrity() {
    let mut history =
        FixedHistory::<u64, MutablePayload, 4, 4, 2>::new(anchor(0, 0, 0, 0), 0, LIMITS).unwrap();
    let value = Rc::new(Cell::new(1));
    history
        .append_input(
            input(0, 1, 1),
            MutablePayload {
                value: Rc::clone(&value),
            },
        )
        .unwrap();
    assert_eq!(history.read(input(0, 1, 1)).unwrap().value.value.get(), 1);
    value.set(99);
    assert!(matches!(
        history.read(input(0, 1, 1)),
        Err(TimelineError::InputMismatch)
    ));
    assert_eq!(history.payload_bytes(), 16);
}

struct CountedState {
    value: u64,
    clones: Rc<Cell<u64>>,
}
impl Clone for CountedState {
    fn clone(&self) -> Self {
        self.clones.set(self.clones.get() + 1);
        Self {
            value: self.value,
            clones: Rc::clone(&self.clones),
        }
    }
}
impl HistoryPayload for CountedState {
    fn digest(&self) -> Digest {
        HistoryPayload::digest(&self.value)
    }
    fn byte_len(&self) -> u64 {
        8
    }
}

#[test]
fn snapshot_capture_checks_slots_and_bytes_before_cloning() {
    let clones = Rc::new(Cell::new(0));
    let state = CountedState {
        value: 0,
        clones: Rc::clone(&clones),
    };
    let mut slots =
        FixedHistory::<_, u64, 2, 1, 1>::new(anchor(0, 0, 0, 0), state, LIMITS).unwrap();
    let current = CountedState {
        value: 1,
        clones: Rc::clone(&clones),
    };
    slots.append_input(input(0, 1, 1), 1).unwrap();
    assert_eq!(
        slots.capture(anchor(0, 1, 1, 1), &current),
        Err(TimelineError::Capacity)
    );
    assert_eq!(clones.get(), 0);
    let byte_state = CountedState {
        value: 0,
        clones: Rc::clone(&clones),
    };
    let limits = HistoryLimits {
        max_total_bytes: 16,
        ..LIMITS
    };
    let mut bytes =
        FixedHistory::<_, u64, 2, 3, 1>::new(anchor(0, 0, 0, 0), byte_state, limits).unwrap();
    bytes.append_input(input(0, 1, 1), 1).unwrap();
    assert_eq!(
        bytes.capture(anchor(0, 1, 1, 1), &current),
        Err(TimelineError::Capacity)
    );
    assert_eq!(clones.get(), 0);
    let original = CountedState {
        value: 0,
        clones: Rc::clone(&clones),
    };
    bytes.capture(anchor(0, 0, 0, 0), &original).unwrap();
    assert_eq!(clones.get(), 0); // Idempotent capture also avoids a clone.
    assert_eq!(bytes.payload_bytes(), 16);
}

#[test]
fn correction_import_refuses_unadmitted_future_inputs() {
    let mut history = history();
    let before = history.payload_bytes();
    assert_eq!(
        history.import_input(input(0, 5, 5), 5),
        Err(TimelineError::HistoryUnavailable)
    );
    assert_eq!(history.payload_bytes(), before);
}

#[test]
fn sparse_anchor_intervals_never_substitute_an_earlier_state_for_input_replacement() {
    let mut history = History::new(anchor(0, 0, 0, 0), 0, LIMITS).unwrap();
    for step in 1..=4 {
        history.append_input(input(0, step, step), step).unwrap();
    }
    history.insert_snapshot(anchor(0, 4, 10, 4), 10).unwrap();
    let mut replacement = input(0, 3, 30);
    replacement.revision = InputRevision::new(1);
    history.import_input(replacement, 30).unwrap();
    assert_eq!(
        history.snapshot_at(head().branch(), TimelineStep::new(2)),
        Err(TimelineError::HistoryUnavailable)
    );
    let mut scratch = [input(0, 1, 1); 16];
    let original = history
        .collect_inputs(
            head().branch(),
            TimelineStep::ZERO,
            head().step(),
            &mut scratch,
        )
        .unwrap();
    assert!(matches!(
        ReplayPlan::prepare(
            head(),
            head().revision(),
            Correction::Input {
                anchor: anchor(0, 0, 0, 0),
                replacement
            },
            original,
            REPLAY
        ),
        Err(TimelineError::HistoryUnavailable)
    ));
    // The application explicitly supplies the complete exact S2. This is not an
    // implicit fallback to S0 or a fabricated snapshot between sparse anchors.
    history.insert_snapshot(anchor(0, 2, 3, 2), 3).unwrap();
    let predecessor = history
        .snapshot_at(head().branch(), TimelineStep::new(2))
        .unwrap();
    let records = history
        .collect_inputs(
            head().branch(),
            predecessor.step,
            head().step(),
            &mut scratch,
        )
        .unwrap();
    let plan = ReplayPlan::prepare(
        head(),
        head().revision(),
        Correction::Input {
            anchor: predecessor,
            replacement,
        },
        records,
        REPLAY,
    )
    .unwrap();
    let mut live = Live::new(head(), 10, EFFECTS);
    let reservation = live
        .reserve(
            &plan,
            observation(head(), replacement.step, replacement.digest, 1),
        )
        .unwrap();
    history.fork(&plan, reservation).unwrap();
    let mut replay = history
        .begin_replay::<(), 4>(plan, reservation, 0, EFFECTS)
        .unwrap();
    replay.advance(head(), &Model, 1).unwrap();
    assert_eq!(*replay.scratch(), 33);
    assert_eq!(live.read(), (head(), &10));
    replay.advance(head(), &Model, 1).unwrap();
    let ready = replay.validate(head(), &Valid).unwrap();
    live.promote(ready).unwrap();
    assert_eq!(*live.read().1, 37);
    assert_eq!(
        history
            .snapshot_at(head().branch(), head().step())
            .unwrap()
            .digest,
        10_u64.digest()
    );
}

#[test]
fn genesis_state_correction_validates_and_publishes_without_running_an_input() {
    let genesis = TimelineHead::new(
        scope(),
        TimelineBranchId::ZERO,
        TimelineStep::ZERO,
        TimelineRevision::ZERO,
        None,
    )
    .unwrap();
    let mut history = History::new(anchor(0, 0, 0, 0), 0, LIMITS).unwrap();
    let corrected = anchor(0, 0, 23, 23);
    history.import_anchor(corrected, 23).unwrap();
    let plan = ReplayPlan::prepare(
        genesis,
        genesis.revision(),
        Correction::State(corrected),
        &[],
        REPLAY,
    )
    .unwrap();
    let mut live = Live::new(genesis, 0, EFFECTS);
    let token = live
        .reserve(
            &plan,
            observation(genesis, corrected.step, corrected.digest, 1),
        )
        .unwrap();
    history.fork(&plan, token).unwrap();
    let session = history
        .begin_replay::<(), 4>(plan, token, 99, EFFECTS)
        .unwrap();
    assert_eq!(session.phase(), ReplayPhase::ReadyToValidate);
    assert_eq!(*session.scratch(), 23);
    assert_eq!(live.read(), (genesis, &0));
    let ready = session
        .validate(
            genesis,
            &DigestValidator {
                expected: 23_u64.digest(),
                digest: HistoryPayload::digest,
            },
        )
        .unwrap();
    let published = live.promote(ready).unwrap();
    assert_eq!(published.step(), TimelineStep::ZERO);
    assert_eq!(published.revision(), TimelineRevision::new(1));
    assert_eq!(published.branch(), token.branch());
    assert_eq!(*live.read().1, 23);
    assert_eq!(
        history
            .snapshot_at(genesis.branch(), genesis.step())
            .unwrap()
            .digest,
        0_u64.digest()
    );
    assert_eq!(published.effect_finalized(), None);
}
