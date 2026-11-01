//! Real scratch execution, failure isolation, poisoned sinks and publication races.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::sync::{Arc, Barrier, RwLock};

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
fn records() -> Vec<InputRecord> {
    (2..=4)
        .map(|step| InputRecord {
            scope: scope(),
            branch: head().branch(),
            step: TimelineStep::new(step),
            identity: TimelineInputId::from_digest(Digest::of(&step.to_le_bytes())),
            revision: InputRevision::ZERO,
            digest: Digest::of(&step.to_le_bytes()),
        })
        .collect()
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
const LIMITS: EffectLimits = EffectLimits {
    max_intents: 8,
    max_payload_bytes: 128,
};
type Live = LiveTimeline<i64, u64, 4, 8>;
struct Snapshots {
    corrected: i64,
    fail: bool,
}
impl SnapshotStore<i64> for Snapshots {
    type Error = ();
    fn restore(&self, anchor: StateAnchor, scratch: &mut i64) -> Result<StateAnchor, ()> {
        *scratch = self.corrected;
        if self.fail {
            return Err(());
        }
        Ok(anchor)
    }
}
impl LiveSnapshotStore<i64> for Snapshots {
    fn restore_current(
        &self,
        expected: TimelineHead,
        anchor: StateAnchor,
        scratch: &mut i64,
    ) -> Result<StateAnchor, ()> {
        if anchor.step != expected.step()
            || anchor.scope != expected.scope()
            || anchor.branch != expected.branch()
        {
            return Err(());
        }
        self.restore(anchor, scratch)
    }
}
struct Inputs {
    values: [u64; 3],
    wrong: bool,
    missing: bool,
}
impl InputStore<u64> for Inputs {
    type Error = ();
    fn read(&self, mut record: InputRecord) -> Result<BoundInput<'_, u64>, ()> {
        if self.missing {
            return Err(());
        }
        let value = self
            .values
            .get(record.step.get().checked_sub(2).unwrap() as usize)
            .ok_or(())?;
        if self.wrong {
            record.digest = Digest([99; 32]);
        }
        Ok(BoundInput { record, value })
    }
}
struct Model {
    fail: bool,
    panic: bool,
    ignore_staging: bool,
}
impl RewindableComputation for Model {
    type State = i64;
    type Input = u64;
    type Effect = u64;
    type Error = TimelineError;
    #[allow(clippy::panic_in_result_fn)] // Deliberate unwind fault injection.
    fn step<S: EffectSink<Effect = u64>>(
        &self,
        state: &mut i64,
        input: &u64,
        context: StepContext,
        sink: &mut S,
    ) -> Result<(), TimelineError> {
        *state += *input as i64;
        if self.panic {
            panic!("injected consumer panic");
        }
        if self.fail {
            return Err(TimelineError::ValidationFailed);
        }
        let result = sink.stage(
            TimelineEffectId::from_digest(Digest::of(&context.step.get().to_le_bytes())),
            Digest::of(&input.to_le_bytes()),
            8,
            *input,
        );
        if self.ignore_staging {
            return Ok(());
        }
        result
    }
}
const MODEL: Model = Model {
    fail: false,
    panic: false,
    ignore_staging: false,
};
const INPUTS: Inputs = Inputs {
    values: [2, 3, 4],
    wrong: false,
    missing: false,
};
struct Valid;
impl CandidateValidator<i64> for Valid {
    fn validate(&self, state: &i64, _head: TimelineHead) -> Result<(), TimelineError> {
        if *state < 0 {
            return Err(TimelineError::ValidationFailed);
        }
        Ok(())
    }
}

#[test]
fn incremental_replay_publishes_only_after_full_validation() {
    let records = records();
    let plan = plan(&records);
    let mut live = Live::new(head(), 10, LIMITS);
    let reservation = live.reserve(&plan, observation(1)).unwrap();
    let mut session = ReplaySession::<_, _, 8>::begin(
        plan,
        reservation,
        0,
        &Snapshots {
            corrected: 100,
            fail: false,
        },
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        session.advance(head(), &MODEL, &INPUTS, 0).unwrap(),
        ReplayPhase::Replaying
    );
    assert_eq!(session.completed_steps(), 0);
    for expected in [102, 105, 109] {
        session.advance(head(), &MODEL, &INPUTS, 1).unwrap();
        assert_eq!(*session.scratch(), expected);
        assert_eq!(live.read(), (head(), &10));
        assert_eq!(live.tentative_effects().count(), 0);
    }
    assert_eq!(session.phase(), ReplayPhase::ReadyToValidate);
    let ready = session.validate(head(), &Valid).unwrap();
    let promoted = live.promote(ready).unwrap();
    assert_eq!(live.read(), (promoted, &109));
    assert_eq!(promoted.branch(), reservation.branch());
    assert_eq!(promoted.revision(), TimelineRevision::new(10));
    assert_eq!(promoted.effect_finalized(), None);
    assert_eq!(live.tentative_effects().count(), 3);
    for intent in live.tentative_effects() {
        let record = intent.descriptor.record();
        assert_eq!(record.branch, promoted.branch());
        assert_eq!(record.revision, promoted.revision());
        let encoded = TimelineRecord::Intent(record);
        let mut buffer = [0; TIMELINE_RECORD_MAX_BYTES];
        let len = encoded.encode(&mut buffer).unwrap();
        assert_eq!(TimelineRecord::decode(&buffer[..len]).unwrap(), encoded);
    }
}
#[test]
fn restore_input_model_and_validator_failures_preserve_live_state() {
    for failure in 0..6 {
        let records = records();
        let plan = plan(&records);
        let mut live = Live::new(head(), 10, LIMITS);
        let reservation = live.reserve(&plan, observation(1)).unwrap();
        let result = ReplaySession::<_, _, 8>::begin(
            plan,
            reservation,
            0,
            &Snapshots {
                corrected: if failure == 5 { -100 } else { 100 },
                fail: failure == 0,
            },
            LIMITS,
        );
        if failure != 0 {
            let mut session = result.unwrap();
            let model = Model {
                fail: failure == 1,
                panic: false,
                ignore_staging: false,
            };
            let inputs = Inputs {
                values: [2, 3, 4],
                wrong: failure == 2,
                missing: failure == 3,
            };
            let outcome =
                session.advance(head(), &model, &inputs, if failure == 4 { 1 } else { 3 });
            if failure <= 3 {
                assert!(outcome.is_err());
                assert_eq!(session.phase(), ReplayPhase::Aborted);
                assert!(session.advance(head(), &MODEL, &INPUTS, 3).is_err());
            }
            assert!(session.validate(head(), &Valid).is_err());
        } else {
            assert!(result.is_err());
        }
        assert_eq!(live.read(), (head(), &10));
        assert_eq!(live.tentative_effects().count(), 0);
        live.abort(reservation).unwrap();
    }
}
#[test]
fn ignored_effect_capacity_failure_and_caught_panic_cannot_promote() {
    for panic in [false, true] {
        let records = records();
        let plan = plan(&records);
        let mut live = Live::new(head(), 10, LIMITS);
        let reservation = live.reserve(&plan, observation(1)).unwrap();
        let mut session = ReplaySession::<_, _, 8>::begin(
            plan,
            reservation,
            0,
            &Snapshots {
                corrected: 100,
                fail: false,
            },
            EffectLimits {
                max_intents: 0,
                max_payload_bytes: 0,
            },
        )
        .unwrap();
        let model = Model {
            fail: false,
            panic,
            ignore_staging: true,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.advance(head(), &model, &INPUTS, 3)
        }));
        if panic {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        assert_eq!(session.phase(), ReplayPhase::Aborted);
        assert!(session.validate(head(), &Valid).is_err());
        assert_eq!(live.read(), (head(), &10));
        live.abort(reservation).unwrap();
    }
}
#[test]
fn same_baseline_digest_mismatch_is_a_validation_failure() {
    let records = records();
    let plan = plan(&records);
    let mut live = Live::new(head(), 10, LIMITS);
    let reservation = live.reserve(&plan, observation(1)).unwrap();
    let mut session = ReplaySession::<_, _, 8>::begin(
        plan,
        reservation,
        0,
        &Snapshots {
            corrected: 100,
            fail: false,
        },
        LIMITS,
    )
    .unwrap();
    session.advance(head(), &MODEL, &INPUTS, 3).unwrap();
    let validator = DigestValidator {
        expected: Digest([0; 32]),
        digest: |state: &i64| Digest::of(&state.to_le_bytes()),
    };
    assert!(session.validate(head(), &validator).is_err());
    assert_eq!(live.read(), (head(), &10));
}
#[test]
fn correction_arriving_mid_replay_supersedes_the_candidate() {
    let records = records();
    let plan = plan(&records);
    let mut live = Live::new(head(), 10, LIMITS);
    let reservation = live.reserve(&plan, observation(1)).unwrap();
    let mut session = ReplaySession::<_, _, 8>::begin(
        plan,
        reservation,
        0,
        &Snapshots {
            corrected: 100,
            fail: false,
        },
        LIMITS,
    )
    .unwrap();
    session.advance(head(), &MODEL, &INPUTS, 1).unwrap();
    let observed = live.observe(head(), observation(2)).unwrap();
    assert!(session.advance(observed, &MODEL, &INPUTS, 3).is_err());
    assert_eq!(session.phase(), ReplayPhase::Aborted);
    assert_eq!(live.read(), (observed, &10));
    assert_eq!(observed.effect_finalized(), None);
}
#[test]
fn competing_promotions_and_readers_never_observe_mixed_state_and_head() {
    for _round in 0..16 {
        let live = Arc::new(RwLock::new(Live::new(head(), 10, LIMITS)));
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for correction in [100, 200] {
            let live = Arc::clone(&live);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                let records = records();
                let plan = plan(&records);
                let reservation = live
                    .write()
                    .unwrap()
                    .reserve(&plan, observation(correction as u64))
                    .unwrap();
                let mut session = ReplaySession::<_, _, 8>::begin(
                    plan,
                    reservation,
                    0,
                    &Snapshots {
                        corrected: correction,
                        fail: false,
                    },
                    LIMITS,
                )
                .unwrap();
                session.advance(head(), &MODEL, &INPUTS, 3).unwrap();
                let ready = session.validate(head(), &Valid).unwrap();
                barrier.wait();
                let result = live.write().unwrap().promote(ready);
                if result.is_err() {
                    live.write().unwrap().abort(reservation).unwrap();
                }
                result.is_ok()
            }));
        }
        barrier.wait();
        for _read in 0..100 {
            let guard = live.read().unwrap();
            let (published, state) = guard.read();
            if published == head() {
                assert_eq!(*state, 10);
            } else {
                assert_eq!(published.revision(), TimelineRevision::new(10));
                assert_eq!(
                    *state,
                    published.authority().unwrap().sequence.get() as i64 + 9
                );
            }
        }
        let wins = workers
            .into_iter()
            .map(|worker| usize::from(worker.join().unwrap()))
            .sum::<usize>();
        assert_eq!(wins, 1);
    }
}

struct Durable {
    admitted: Vec<EffectDescriptor>,
    calls: usize,
    lose_ack: bool,
    wrong_receipt: bool,
}
impl EffectHandoff<u64> for Durable {
    type Error = ();
    #[allow(clippy::panic_in_result_fn)]
    fn submit(&mut self, effect: EligibleEffect<'_, u64>) -> Result<EffectAdmissionReceipt, ()> {
        self.calls += 1;
        let descriptor = effect.descriptor();
        if let Some(old) = self
            .admitted
            .iter()
            .find(|old| old.identity == descriptor.identity)
        {
            assert_eq!(*old, descriptor);
        } else {
            self.admitted.push(descriptor);
        }
        assert_eq!(
            descriptor.digest,
            Digest::of(&effect.payload().to_le_bytes())
        );
        if self.lose_ack {
            self.lose_ack = false;
            return Err(());
        }
        let mut receipt = EffectAdmissionReceipt {
            descriptor,
            durable_commit: Digest([44; 32]),
        };
        if self.wrong_receipt {
            receipt.descriptor.digest = Digest([55; 32]);
        }
        Ok(receipt)
    }
}
fn corrected_live() -> Live {
    let records = records();
    let plan = plan(&records);
    let mut live = Live::new(head(), 10, LIMITS);
    let reservation = live.reserve(&plan, observation(1)).unwrap();
    let mut session = ReplaySession::<_, _, 8>::begin(
        plan,
        reservation,
        0,
        &Snapshots {
            corrected: 100,
            fail: false,
        },
        LIMITS,
    )
    .unwrap();
    session.advance(head(), &MODEL, &INPUTS, 3).unwrap();
    live.promote(session.validate(head(), &Valid).unwrap())
        .unwrap();
    live
}
#[test]
fn effects_require_independent_finality_and_lost_ack_retries_keep_exact_identity() {
    let mut live = corrected_live();
    let mut durable = Durable {
        admitted: Vec::new(),
        calls: 0,
        lose_ack: true,
        wrong_receipt: false,
    };
    assert_eq!(live.eligible_effects().count(), 0);
    assert_eq!(live.handoff(&mut durable, 8).unwrap(), 0);
    assert_eq!(durable.calls, 0);
    let observed = live.read().0;
    let mut next = observation(2);
    next.step = TimelineStep::new(4);
    let accepted = live.observe(observed, next).unwrap();
    assert_eq!(live.handoff(&mut durable, 8).unwrap(), 0);
    assert_eq!(durable.calls, 0);
    let finalized = live
        .finalize_effects(accepted, TimelineStep::new(2))
        .unwrap();
    assert_eq!(live.eligible_effects().count(), 1);
    assert!(live.handoff(&mut durable, 8).is_err());
    assert_eq!(live.eligible_effects().count(), 1);
    assert_eq!(durable.admitted.len(), 1);
    assert_eq!(live.handoff(&mut durable, 8).unwrap(), 1);
    assert_eq!(durable.admitted.len(), 1);
    assert_eq!(live.handoff(&mut durable, 8).unwrap(), 0);
    live.finalize_effects(finalized, TimelineStep::new(4))
        .unwrap();
    assert_eq!(live.handoff(&mut durable, 1).unwrap(), 1);
    assert_eq!(live.handoff(&mut durable, 8).unwrap(), 1);
    assert_eq!(durable.admitted.len(), 3);
    assert_eq!(live.tentative_effects().count(), 0);
    assert_eq!(durable.calls, 4);
}
#[test]
fn wrong_durable_receipt_never_retires_an_intent() {
    let mut live = corrected_live();
    let current = live.read().0;
    live.finalize_effects(current, TimelineStep::new(4))
        .unwrap();
    let mut durable = Durable {
        admitted: Vec::new(),
        calls: 0,
        lose_ack: false,
        wrong_receipt: true,
    };
    assert!(live.handoff(&mut durable, 8).is_err());
    assert_eq!(live.eligible_effects().count(), 3);
    durable.wrong_receipt = false;
    assert_eq!(live.handoff(&mut durable, 8).unwrap(), 3);
    assert_eq!(durable.admitted.len(), 3);
}

struct FifthInput;
impl InputStore<u64> for FifthInput {
    type Error = ();
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, u64>, ()> {
        Ok(BoundInput { record, value: &5 })
    }
}
fn ready_fifth() -> ReadyStep<i64, u64, 8> {
    let mut current = anchor();
    current.step = head().step();
    let mut input = records()[0];
    input.step = TimelineStep::new(5);
    input.digest = Digest::of(&5_u64.to_le_bytes());
    prepare_live_step(
        LiveStepRequest {
            expected: head(),
            anchor: current,
            input,
            scratch: 0,
            limits: LIMITS,
        },
        &MODEL,
        &FifthInput,
        &Snapshots {
            corrected: 10,
            fail: false,
        },
        &Valid,
    )
    .unwrap()
}
#[test]
fn live_step_publication_supersedes_replay_without_losing_new_input() {
    let records = records();
    let plan = plan(&records);
    let mut live = Live::new(head(), 10, LIMITS);
    let reservation = live.reserve(&plan, observation(1)).unwrap();
    let mut session = ReplaySession::<_, _, 8>::begin(
        plan,
        reservation,
        0,
        &Snapshots {
            corrected: 100,
            fail: false,
        },
        LIMITS,
    )
    .unwrap();
    session.advance(head(), &MODEL, &INPUTS, 3).unwrap();
    let ready = session.validate(head(), &Valid).unwrap();
    let advanced = live.append(ready_fifth()).unwrap();
    assert_eq!(live.read(), (advanced, &15));
    assert_eq!(advanced.step(), TimelineStep::new(5));
    assert!(live.promote(ready).is_err());
    assert_eq!(live.read(), (advanced, &15));
    live.abort(reservation).unwrap();
}
#[test]
fn live_step_merged_quota_and_duplicate_admission_do_not_mutate_publication() {
    let mut live = Live::new(
        head(),
        10,
        EffectLimits {
            max_intents: 0,
            max_payload_bytes: 0,
        },
    );
    assert!(live.append(ready_fifth()).is_err());
    assert_eq!(live.read(), (head(), &10));
    let mut admitted_live = Live::new(head(), 10, LIMITS);
    let next = admitted_live.append(ready_fifth()).unwrap();
    assert!(admitted_live.append(ready_fifth()).is_err());
    assert_eq!(admitted_live.read(), (next, &15));
}

#[cfg(feature = "std")]
#[test]
fn hosted_profile_uses_one_publication_lock_and_fails_closed_on_poison() {
    let shared = SharedTimeline::new(Live::new(head(), 10, LIMITS));
    assert_eq!(shared.read().unwrap().read(), (head(), &10));
    let writer = shared.clone();
    let failure = std::thread::spawn(move || {
        let _guard = writer.write().unwrap();
        panic!("injected lock poison");
    })
    .join();
    assert!(failure.is_err());
    assert!(matches!(
        shared.read(),
        Err(TimelineError::PublicationUnavailable)
    ));
    assert!(matches!(
        shared.write(),
        Err(TimelineError::PublicationUnavailable)
    ));
}

struct DropState {
    value: i64,
    fail_drop: bool,
}
impl Drop for DropState {
    fn drop(&mut self) {
        if self.fail_drop {
            panic!("injected retired-state destructor panic");
        }
    }
}
struct DropSnapshots;
impl SnapshotStore<DropState> for DropSnapshots {
    type Error = ();
    fn restore(&self, anchor: StateAnchor, state: &mut DropState) -> Result<StateAnchor, ()> {
        state.value = 100;
        Ok(anchor)
    }
}
struct DropModel;
impl RewindableComputation for DropModel {
    type State = DropState;
    type Input = u64;
    type Effect = ();
    type Error = ();
    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut DropState,
        input: &u64,
        _context: StepContext,
        _sink: &mut S,
    ) -> Result<(), ()> {
        state.value += *input as i64;
        Ok(())
    }
}
struct DropValid;
impl CandidateValidator<DropState> for DropValid {
    fn validate(&self, _state: &DropState, _head: TimelineHead) -> Result<(), TimelineError> {
        Ok(())
    }
}
#[test]
fn retired_state_destructor_runs_after_complete_atomic_publication() {
    let records = records();
    let plan = plan(&records);
    let mut live = LiveTimeline::<_, (), 4, 8>::new(
        head(),
        DropState {
            value: 10,
            fail_drop: true,
        },
        LIMITS,
    );
    let reservation = live.reserve(&plan, observation(1)).unwrap();
    let mut session = ReplaySession::<_, _, 8>::begin(
        plan,
        reservation,
        DropState {
            value: 0,
            fail_drop: false,
        },
        &DropSnapshots,
        LIMITS,
    )
    .unwrap();
    session.advance(head(), &DropModel, &INPUTS, 3).unwrap();
    let ready = session.validate(head(), &DropValid).unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| live.promote(ready)));
    assert!(outcome.is_err());
    let (published, state) = live.read();
    assert_eq!(published.branch(), reservation.branch());
    assert_eq!(published.revision(), TimelineRevision::new(10));
    assert_eq!(state.value, 109);
}

const fn scoped_request(records: &[InputRecord]) -> CorrectionRequest<'_, i64> {
    CorrectionRequest {
        correction: Correction::State(anchor()),
        inputs: records,
        observation: observation(1),
        scratch: 0,
        limits: ReplayLimits {
            max_retained_inputs: 8,
            max_replay_steps: 8,
        },
    }
}

#[test]
fn scoped_correction_cleans_every_return_path_with_one_reservation_slot() {
    let records = records();
    let mut live = LiveTimeline::<i64, u64, 1, 8>::new(head(), 10, LIMITS);
    let snapshots = Snapshots {
        corrected: 100,
        fail: false,
    };
    let reject = |_state: &i64, _head: TimelineHead| Err(TimelineError::ValidationFailed);
    for _ in 0..4 {
        assert!(
            live.begin_correction(
                scoped_request(&records),
                &Snapshots {
                    corrected: 100,
                    fail: true
                }
            )
            .is_err()
        );
        // Explicit cancellation needs no token and does not exhaust the one slot.
        drop(
            live.begin_correction(scoped_request(&records), &snapshots)
                .unwrap(),
        );
        assert_eq!(
            live.begin_correction(scoped_request(&records), &snapshots)
                .unwrap()
                .publish(&Valid),
            Err(TimelineError::InvalidPhase)
        );
        let mut correction = live
            .begin_correction(scoped_request(&records), &snapshots)
            .unwrap();
        assert_eq!(
            correction.advance(&MODEL, &INPUTS, 0).unwrap(),
            ReplayPhase::Replaying
        );
        assert_eq!(correction.read(), (head(), &10));
        assert_eq!(
            correction.advance(&MODEL, &INPUTS, 3).unwrap(),
            ReplayPhase::ReadyToValidate
        );
        assert_eq!(
            correction.publish(&reject),
            Err(TimelineError::ValidationFailed)
        );
        assert_eq!(live.read(), (head(), &10));
        assert_eq!(live.tentative_effects().count(), 0);
    }
    let mut correction = live
        .begin_correction(scoped_request(&records), &snapshots)
        .unwrap();
    assert_eq!(
        correction.advance(&MODEL, &INPUTS, 1).unwrap(),
        ReplayPhase::Replaying
    );
    assert_eq!(
        correction.advance(&MODEL, &INPUTS, 2).unwrap(),
        ReplayPhase::ReadyToValidate
    );
    let selected = correction.publish(&Valid).unwrap();
    assert_eq!(live.read(), (selected, &109));
    assert_eq!(live.tentative_effects().count(), 3);
    // Every cancelled reservation consumed an identity, even though it freed a slot.
    assert!(selected.branch().get() > head().branch().get() + 16);
}

#[test]
fn scoped_correction_cleans_restore_model_and_validation_unwinds() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    struct PanickingSnapshot;
    impl SnapshotStore<i64> for PanickingSnapshot {
        type Error = ();
        #[allow(clippy::panic_in_result_fn)]
        fn restore(&self, _anchor: StateAnchor, _state: &mut i64) -> Result<StateAnchor, ()> {
            panic!("restore failure");
        }
    }
    let records = records();
    let snapshots = Snapshots {
        corrected: 100,
        fail: false,
    };
    let mut live = LiveTimeline::<i64, u64, 1, 8>::new(head(), 10, LIMITS);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _candidate = live.begin_correction(scoped_request(&records), &PanickingSnapshot);
        }))
        .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let mut correction = live
                .begin_correction(scoped_request(&records), &snapshots)
                .unwrap();
            let _result = correction.advance(
                &Model {
                    fail: false,
                    panic: true,
                    ignore_staging: false,
                },
                &INPUTS,
                1,
            );
        }))
        .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let mut correction = live
                .begin_correction(scoped_request(&records), &snapshots)
                .unwrap();
            correction.advance(&MODEL, &INPUTS, 3).unwrap();
            let _result = correction.publish(&|_state: &i64,
                                               _head: TimelineHead|
             -> Result<(), TimelineError> {
                panic!("validator failure");
            });
        }))
        .is_err()
    );
    assert_eq!(live.read(), (head(), &10));
    drop(
        live.begin_correction(scoped_request(&records), &snapshots)
            .unwrap(),
    );
}

#[test]
fn scoped_correction_failed_step_cannot_publish_and_releases_slot_on_drop() {
    let records = records();
    let snapshots = Snapshots {
        corrected: 100,
        fail: false,
    };
    let mut live = LiveTimeline::<i64, u64, 1, 0>::new(head(), 10, LIMITS);
    let mut correction = live
        .begin_correction(scoped_request(&records), &snapshots)
        .unwrap();
    assert!(correction.advance(&MODEL, &INPUTS, 1).is_err());
    assert_eq!(correction.publish(&Valid), Err(TimelineError::InvalidPhase));
    assert_eq!(live.read(), (head(), &10));
    drop(
        live.begin_correction(scoped_request(&records), &snapshots)
            .unwrap(),
    );
}

#[test]
fn owned_input_application_derives_metadata_and_preserves_failure_isolation() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let mut live = Live::new(head(), 10, LIMITS);
    let identity = TimelineInputId::from_digest(Digest([88; 32]));
    let value = 5_u64;
    let reject = |_state: &i64, _head: TimelineHead| Err(TimelineError::ValidationFailed);
    assert!(live.apply_input(&MODEL, identity, &value, &reject).is_err());
    assert!(
        live.apply_input(
            &Model {
                fail: true,
                panic: false,
                ignore_staging: false
            },
            identity,
            &value,
            &Valid
        )
        .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _result = live.apply_input(
                &Model {
                    fail: false,
                    panic: true,
                    ignore_staging: false,
                },
                identity,
                &value,
                &Valid,
            );
        }))
        .is_err()
    );
    assert_eq!(live.read(), (head(), &10));
    assert_eq!(live.tentative_effects().count(), 0);
    let record = live
        .apply_input(
            &MODEL,
            identity,
            &value,
            &|state: &i64, next: TimelineHead| {
                assert_eq!(*state, 15);
                assert_eq!(next.step().get(), 5);
                assert_eq!(next.revision().get(), 10);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        record,
        InputRecord {
            scope: scope(),
            branch: head().branch(),
            step: TimelineStep::new(5),
            identity,
            revision: InputRevision::ZERO,
            digest: value.digest()
        }
    );
    assert_eq!(*live.read().1, 15);
    assert_eq!(live.tentative_effects().count(), 1);
    let mut no_effects = LiveTimeline::<i64, u64, 1, 0>::new(head(), 10, LIMITS);
    assert!(
        no_effects
            .apply_input(
                &Model {
                    fail: false,
                    panic: false,
                    ignore_staging: true
                },
                identity,
                &value,
                &Valid
            )
            .is_err()
    );
    assert_eq!(no_effects.read(), (head(), &10));
}

#[test]
fn captured_adapters_bind_scope_version_and_payload_on_every_read() {
    use std::cell::Cell;
    struct MutablePayload(Cell<u64>);
    impl HistoryPayload for MutablePayload {
        fn digest(&self) -> Digest {
            self.0.get().digest()
        }
        fn byte_len(&self) -> u64 {
            8
        }
    }
    let mut record = records()[0];
    record.digest = 2_u64.digest();
    assert!(CapturedInput::new(record, 3_u64).is_err());
    let input = CapturedInput::new(record, 2_u64).unwrap();
    assert_eq!(input.record(), record);
    assert_eq!(*input.read(record).unwrap().value, 2);
    let inputs = [input];
    assert_eq!(*inputs.read(record).unwrap().value, 2);
    let mut wrong = record;
    wrong.revision = InputRevision::new(1);
    assert!(inputs.read(wrong).is_err());
    let payload = MutablePayload(Cell::new(2));
    let captured = CapturedInput::new(record, payload).unwrap();
    // Deliberately violate the immutability contract: revalidation still refuses it.
    captured.read(record).unwrap().value.0.set(3);
    assert!(captured.read(record).is_err());
    let snapshot =
        CapturedSnapshot::capture(head(), TimelineStep::new(1), anchor().snapshot, 42_u64).unwrap();
    let mut scratch = 0;
    snapshot.restore(snapshot.anchor(), &mut scratch).unwrap();
    assert_eq!(scratch, 42);
    let mut wrong_anchor = snapshot.anchor();
    wrong_anchor.branch = TimelineBranchId::new(99);
    assert!(snapshot.restore(wrong_anchor, &mut scratch).is_err());
    assert_eq!(scratch, 42);
    assert!(
        CapturedSnapshot::capture(head(), TimelineStep::new(5), anchor().snapshot, 42_u64).is_err()
    );
}

#[test]
fn scoped_publication_unwind_preserves_coherence_without_panicking_during_cleanup() {
    let records = records();
    let mut live = LiveTimeline::<DropState, (), 1, 8>::new(
        head(),
        DropState {
            value: 10,
            fail_drop: true,
        },
        LIMITS,
    );
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut correction = live
            .begin_correction(
                CorrectionRequest {
                    correction: Correction::State(anchor()),
                    inputs: &records,
                    observation: observation(1),
                    scratch: DropState {
                        value: 0,
                        fail_drop: false,
                    },
                    limits: ReplayLimits {
                        max_retained_inputs: 8,
                        max_replay_steps: 8,
                    },
                },
                &DropSnapshots,
            )
            .unwrap();
        correction.advance(&DropModel, &INPUTS, 3).unwrap();
        correction.publish(&DropValid)
    }));
    assert!(outcome.is_err());
    assert_eq!(live.read().0.revision(), TimelineRevision::new(10));
    assert_eq!(live.read().1.value, 109);
}

#[test]
fn owned_input_rejects_merged_effect_quota_and_counter_exhaustion() {
    let identity = TimelineInputId::from_digest(Digest([88; 32]));
    let mut live = LiveTimeline::<i64, u64, 1, 1>::new(head(), 10, LIMITS);
    live.apply_input(&MODEL, identity, &5, &Valid).unwrap();
    let accepted = live.read().0;
    assert!(live.apply_input(&MODEL, identity, &6, &Valid).is_err());
    assert_eq!(live.read(), (accepted, &15));
    assert_eq!(live.tentative_effects().count(), 1);
    for exhausted in [
        TimelineHead::new(
            scope(),
            head().branch(),
            TimelineStep::new(u64::MAX),
            head().revision(),
            None,
        )
        .unwrap(),
        TimelineHead::new(
            scope(),
            head().branch(),
            head().step(),
            TimelineRevision::new(u64::MAX),
            None,
        )
        .unwrap(),
    ] {
        let mut exhausted_live = Live::new(exhausted, 10, LIMITS);
        assert!(matches!(
            exhausted_live.apply_input(&MODEL, identity, &5, &Valid),
            Err(ApplyError::Timeline(TimelineError::CounterExhausted))
        ));
        assert_eq!(exhausted_live.read(), (exhausted, &10));
    }
}

#[test]
fn all_correction_validators_receive_the_exact_proposed_publication_head() {
    use std::cell::Cell;
    let records = records();
    let snapshots = Snapshots {
        corrected: 100,
        fail: false,
    };
    for scoped in [false, true] {
        let mut live = LiveTimeline::<i64, u64, 1, 8>::new(head(), 10, LIMITS);
        let validated = Cell::new(None);
        let validate = |state: &i64, proposed: TimelineHead| {
            assert_eq!(*state, 109);
            assert_eq!(proposed.scope(), head().scope());
            assert_eq!(proposed.step(), head().step());
            assert_eq!(
                proposed.revision(),
                head().revision().checked_next().unwrap()
            );
            assert!(proposed.branch() > head().branch());
            assert_eq!(proposed.authority(), Some(observation(1)));
            assert_eq!(proposed.effect_finalized(), head().effect_finalized());
            validated.set(Some(proposed));
            Ok(())
        };
        let published = if scoped {
            let mut correction = live
                .begin_correction(scoped_request(&records), &snapshots)
                .unwrap();
            correction.advance(&MODEL, &INPUTS, 3).unwrap();
            correction.publish(&validate).unwrap()
        } else {
            let plan = plan(&records);
            let token = live.reserve(&plan, observation(1)).unwrap();
            let mut session = ReplaySession::begin(plan, token, 0, &snapshots, LIMITS).unwrap();
            session.advance(head(), &MODEL, &INPUTS, 3).unwrap();
            live.promote(session.validate(head(), &validate).unwrap())
                .unwrap()
        };
        assert_eq!(validated.get(), Some(published));
        assert_eq!(live.read(), (published, &109));
    }
}
