//! Shared bounded trace oracles for proptest, exhaustive tests and libFuzzer.
//! State arithmetic is deliberately modulo 2^64; protocol counters never wrap.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::collections::BTreeMap;

const FX: EffectLimits = EffectLimits {
    max_intents: 32,
    max_payload_bytes: 256,
};
const LIMITS: ReplayLimits = ReplayLimits {
    max_retained_inputs: 32,
    max_replay_steps: 32,
};
type History = FixedHistory<u64, u64, 64, 64, 8>;
type Live = LiveTimeline<u64, u64, 3, 32>;
fn byte(data: &[u8], offset: usize) -> u8 {
    data.get(offset).copied().unwrap_or(0)
}
const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
fn anchor(branch: TimelineBranchId, step: u64, state: u64) -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch,
        step: TimelineStep::new(step),
        snapshot: TimelineSnapshotId::from_digest(state.digest()),
        digest: state.digest(),
    }
}
fn input(branch: TimelineBranchId, step: u64, value: u64) -> InputRecord {
    InputRecord {
        scope: scope(),
        branch,
        step: TimelineStep::new(step),
        identity: TimelineInputId::from_digest(Digest::of(&step.to_le_bytes())),
        revision: InputRevision::ZERO,
        digest: value.digest(),
    }
}
const fn observation(step: TimelineStep, digest: Digest, sequence: u64) -> AuthorityObservation {
    AuthorityObservation {
        scope: scope(),
        authority: TimelineAuthorityId::from_digest(Digest([5; 32])),
        epoch: AuthorityEpoch::ZERO,
        sequence: AuthoritySequence::new(sequence),
        step,
        digest,
    }
}
fn head(step: u64) -> TimelineHead {
    TimelineHead::new(
        scope(),
        TimelineBranchId::ZERO,
        TimelineStep::new(step),
        TimelineRevision::ZERO,
        None,
    )
    .unwrap()
}
// Closed form, rather than calling/repeating the consumer's iterative step.
fn oracle(initial: u64, values: &[u64]) -> u64 {
    let count = u32::try_from(values.len()).unwrap();
    values.iter().enumerate().fold(
        initial.wrapping_mul(3_u64.wrapping_pow(count)),
        |sum, (offset, value)| {
            sum.wrapping_add(
                value.wrapping_mul(3_u64.wrapping_pow(count - 1 - u32::try_from(offset).unwrap())),
            )
        },
    )
}
fn validator(expected: u64) -> DigestValidator<fn(&u64) -> Digest> {
    DigestValidator {
        expected: expected.digest(),
        digest: u64::digest,
    }
}
struct Model {
    fail_at: Option<u64>,
    duplicate_at: Option<u64>,
}
impl RewindableComputation for Model {
    type State = u64;
    type Input = u64;
    type Effect = u64;
    type Error = ();
    fn step<S: EffectSink<Effect = u64>>(
        &self,
        state: &mut u64,
        input: &u64,
        context: StepContext,
        sink: &mut S,
    ) -> Result<(), ()> {
        *state = state.wrapping_mul(3).wrapping_add(*input);
        if self.fail_at == Some(context.step.get()) {
            return Err(());
        }
        let identity = TimelineEffectId::from_digest(Digest::of(&context.step.get().to_le_bytes()));
        let accepted = sink.stage(identity, input.digest(), 8, *input);
        if self.duplicate_at == Some(context.step.get()) {
            let _ignored = sink.stage(identity, input.digest(), 8, *input);
        }
        accepted.map_err(|_error| ())
    }
}
const MODEL: Model = Model {
    fail_at: None,
    duplicate_at: None,
};
struct Fixture {
    history: History,
    live: Live,
    initial: u64,
    values: Vec<u64>,
}
impl Fixture {
    fn new(data: &[u8]) -> Self {
        let count = usize::from(byte(data, 0) % 16) + 1;
        let initial = u64::from(byte(data, 1)).wrapping_mul(0x0101_0101_0101_0101);
        let values: Vec<_> = (0..count)
            .map(|offset| u64::from(byte(data, offset + 8)))
            .collect();
        let genesis = head(0);
        let mut history = History::new(
            anchor(genesis.branch(), 0, initial),
            initial,
            HistoryLimits {
                max_input_bytes: 8,
                max_snapshot_bytes: 8,
                max_total_bytes: 4096,
            },
        )
        .unwrap();
        let mut live = Live::new(genesis, initial, FX);
        for (offset, value) in values.iter().copied().enumerate() {
            append(
                &mut history,
                &mut live,
                value,
                oracle(initial, &values[..=offset]),
            );
        }
        for effect in live.tentative_effects() {
            let step = effect.descriptor.origin.step.get();
            assert!(step > 0 && step <= u64::try_from(count).unwrap());
            assert_eq!(effect.descriptor.origin.scope, scope());
            assert_eq!(effect.descriptor.origin.branch, genesis.branch());
            assert_eq!(effect.descriptor.origin.revision.get(), step);
            assert_eq!(
                effect.descriptor.identity,
                TimelineEffectId::from_digest(Digest::of(&step.to_le_bytes()))
            );
            assert_eq!(effect.payload, values[usize::try_from(step - 1).unwrap()]);
            assert_eq!(effect.descriptor.digest, effect.payload.digest());
        }
        Self {
            history,
            live,
            initial,
            values,
        }
    }
}
fn append(history: &mut History, live: &mut Live, value: u64, expected: u64) {
    let current = live.read().0;
    let record = input(current.branch(), current.step().get() + 1, value);
    history.append_input(record, value).unwrap();
    let ready = prepare_live_step::<_, _, _, _, 32>(
        LiveStepRequest {
            expected: current,
            anchor: history
                .snapshot_at(current.branch(), current.step())
                .unwrap(),
            input: record,
            scratch: 0,
            limits: FX,
        },
        &MODEL,
        history,
        history,
        &validator(expected),
    )
    .unwrap();
    let advanced = live.append(ready).unwrap();
    assert_eq!(*live.read().1, expected);
    history
        .insert_snapshot(
            anchor(advanced.branch(), advanced.step().get(), expected),
            expected,
        )
        .unwrap();
}
fn records(branch: TimelineBranchId, after: u64, values: &[u64]) -> Vec<InputRecord> {
    values
        .iter()
        .copied()
        .enumerate()
        .skip(usize::try_from(after).unwrap())
        .map(|(offset, value)| input(branch, u64::try_from(offset + 1).unwrap(), value))
        .collect()
}

/// Legal and illegal metadata transitions versus an independent state machine.
pub fn projection(data: &[u8]) {
    struct Reservation {
        token: BranchReservation,
        branch: u64,
        revision: u64,
        observation: AuthorityObservation,
        active: bool,
    }
    let mut actual = TimelineProjection::<3>::new(head(0));
    let (mut branch, mut step, mut revision, mut high) = (0, 0, 0, 0);
    let mut finality = None;
    let mut authority: Option<AuthorityObservation> = None;
    let mut saved: Vec<Reservation> = Vec::new();
    for pair in data.chunks(2).take(64) {
        let parameter = u64::from(byte(pair, 1));
        let before = actual.head();
        match byte(pair, 0) % 8 {
            0 => {
                actual
                    .append(
                        before,
                        input(TimelineBranchId::new(branch), step + 1, parameter),
                    )
                    .unwrap();
                step += 1;
                revision += 1;
            }
            1 => {
                let mut observed =
                    observation(TimelineStep::new(step), parameter.digest(), parameter % 32);
                observed.epoch = AuthorityEpoch::new(parameter / 32);
                if parameter & 128 != 0 {
                    observed.authority = TimelineAuthorityId::from_digest(Digest([9; 32]));
                }
                let valid = authority.is_none_or(|old| {
                    old.authority == observed.authority
                        && (old.epoch.get(), old.sequence.get())
                            < (observed.epoch.get(), observed.sequence.get())
                });
                let result = actual.observe(before, observed);
                assert_eq!(result.is_ok(), valid);
                if valid {
                    authority = Some(observed);
                    revision += 1;
                }
            }
            2 => {
                let frontier = parameter % (step + 2);
                let valid = frontier <= step && finality.is_none_or(|old| frontier > old);
                assert_eq!(
                    actual
                        .finalize_effects(before, TimelineStep::new(frontier))
                        .is_ok(),
                    valid
                );
                if valid {
                    finality = Some(frontier);
                    revision += 1;
                }
            }
            3 => {
                let corrected = parameter % (step + 1);
                let values = vec![0; usize::try_from(step).unwrap()];
                let inputs = records(before.branch(), corrected, &values);
                let corrected_anchor = anchor(before.branch(), corrected, parameter);
                let plan = ReplayPlan::prepare(
                    before,
                    before.revision(),
                    Correction::State(corrected_anchor),
                    &inputs,
                    ReplayLimits {
                        max_retained_inputs: 64,
                        max_replay_steps: 64,
                    },
                );
                if finality.is_some_and(|frontier| corrected <= frontier) {
                    assert!(matches!(plan, Err(TimelineError::ReconciliationRequired)));
                } else {
                    let planned = plan.unwrap();
                    let mut observed =
                        observation(corrected_anchor.step, corrected_anchor.digest, 1);
                    if let Some(old) = authority {
                        observed.authority = old.authority;
                        observed.epoch = old.epoch;
                        observed.sequence = AuthoritySequence::new(old.sequence.get() + 1);
                    }
                    let free = saved.iter().filter(|entry| entry.active).count() < 3;
                    let result = actual.reserve(&planned, observed);
                    assert_eq!(result.is_ok(), free);
                    if let Ok(token) = result {
                        high += 1;
                        assert_eq!(token.branch().get(), high);
                        saved.push(Reservation {
                            token,
                            branch: high,
                            revision,
                            observation: observed,
                            active: true,
                        });
                    }
                }
            }
            4 | 5 => {
                if !saved.is_empty() {
                    let selected = usize::try_from(parameter).unwrap() % saved.len();
                    let entry = &mut saved[selected];
                    if byte(pair, 0) % 8 == 4 {
                        let result = actual.promote(entry.token);
                        if !entry.active {
                            assert_eq!(result, Err(TimelineError::SessionUnavailable));
                        } else if entry.revision != revision {
                            assert_eq!(result, Err(TimelineError::Conflict));
                        } else {
                            result.unwrap();
                            branch = entry.branch;
                            revision += 1;
                            authority = Some(entry.observation);
                            entry.active = false;
                        }
                    } else {
                        let result = actual.abort(entry.token);
                        assert_eq!(result.is_ok(), entry.active);
                        entry.active = false;
                    }
                }
            }
            6 => {
                actual = TimelineProjection::restore(before, TimelineBranchId::new(high)).unwrap();
                for entry in &mut saved {
                    entry.active = false;
                }
            }
            _ => {
                let mut foreign =
                    observation(TimelineStep::new(step), parameter.digest(), parameter);
                foreign.scope.schema = TimelineSchemaId::from_digest(Digest([99; 32]));
                assert_eq!(
                    actual.observe(before, foreign),
                    Err(TimelineError::ScopeMismatch)
                );
            }
        }
        let current = actual.head();
        assert_eq!(
            (
                current.branch().get(),
                current.step().get(),
                current.revision().get()
            ),
            (branch, step, revision)
        );
        assert_eq!(current.effect_finalized().map(TimelineStep::get), finality);
        assert_eq!(current.authority(), authority);
        assert_eq!(actual.last_branch().get(), high);
        assert_eq!(
            actual.active_sessions(),
            saved.iter().filter(|entry| entry.active).count()
        );
    }
}

/// Precise plan indexing plus malformed scopes, ranges and immutable versions.
pub fn plans(data: &[u8]) {
    let count = u64::from(byte(data, 0) % 16) + 1;
    let input_correction = byte(data, 1) & 1 != 0;
    let corrected = if input_correction {
        u64::from(byte(data, 2)) % count + 1
    } else {
        u64::from(byte(data, 2)) % (count + 1)
    };
    let predecessor = corrected - u64::from(input_correction);
    let mut current = head(count);
    let mut base = anchor(current.branch(), predecessor, 7);
    let values = vec![1; usize::try_from(count).unwrap()];
    let mut original = records(current.branch(), predecessor, &values);
    let mut replacement = input(current.branch(), corrected, 99);
    replacement.revision = InputRevision::new(1);
    let mut expected_revision = current.revision();
    let mut limits = LIMITS;
    let mut valid = false;
    match byte(data, 3) % 16 {
        0 => valid = true,
        1 => {
            if let Some(first) = original.first_mut() {
                first.step = TimelineStep::ZERO;
            } else {
                original.push(input(current.branch(), count + 1, 1));
            }
        }
        2 => {
            let role = byte(data, 4) % 4;
            match role {
                0 => base.scope.tenant = TimelineTenantId::from_digest(Digest([99; 32])),
                1 => base.scope.timeline = TimelineId::from_digest(Digest([99; 32])),
                2 => base.scope.model = TimelineModelId::from_digest(Digest([99; 32])),
                _ => base.scope.schema = TimelineSchemaId::from_digest(Digest([99; 32])),
            }
        }
        3 => base.branch = TimelineBranchId::new(99),
        4 => {
            if original.pop().is_none() {
                original.push(input(current.branch(), count + 1, 1));
            }
        }
        5 => {
            if original.len() > 1 {
                original[1] = original[0];
            } else {
                original.push(input(current.branch(), count + 1, 1));
            }
        }
        6 => original.push(input(current.branch(), count + 1, 1)),
        7 => {
            current = TimelineHead::new(
                scope(),
                current.branch(),
                current.step(),
                current.revision(),
                Some(TimelineStep::new(corrected)),
            )
            .unwrap()
        }
        8 => {
            limits.max_replay_steps = 0;
            valid = original.is_empty();
        }
        9 => expected_revision = TimelineRevision::new(99),
        10 => base.step = TimelineStep::new(count + 1),
        11 => {
            if input_correction {
                replacement.identity = TimelineInputId::from_digest(Digest([99; 32]));
            } else {
                base.scope.schema = TimelineSchemaId::from_digest(Digest([99; 32]));
            }
        }
        12 => {
            if input_correction {
                replacement.revision = InputRevision::ZERO;
            } else {
                base.branch = TimelineBranchId::new(99);
            }
        }
        13 => {
            if input_correction {
                replacement.branch = TimelineBranchId::new(99);
            } else {
                base.step = TimelineStep::new(count + 1);
            }
        }
        14 => {
            if input_correction {
                replacement.step = TimelineStep::new(count + 1);
            } else {
                limits.max_retained_inputs = 0;
                valid = original.is_empty();
            }
        }
        _ => {
            if let Some(first) = original.first_mut() {
                first.scope.schema = TimelineSchemaId::from_digest(Digest([99; 32]));
            } else {
                original.push(input(current.branch(), count + 1, 1));
            }
        }
    }
    let frozen = original.clone();
    let correction = if input_correction {
        Correction::Input {
            anchor: base,
            replacement,
        }
    } else {
        Correction::State(base)
    };
    let result = ReplayPlan::prepare(current, expected_revision, correction, &original, limits);
    assert_eq!(result.is_ok(), valid, "plan case {:?}", data);
    if let Ok(plan) = result {
        assert_eq!(plan.len(), usize::try_from(count - predecessor).unwrap());
        for (offset, record) in original.iter().copied().enumerate() {
            assert_eq!(
                plan.input(offset),
                Some(if input_correction && offset == 0 {
                    replacement
                } else {
                    record
                })
            );
        }
        assert_eq!(plan.input(plan.len()), None);
    }
    assert_eq!(original, frozen);
}

/// Real state/input corrections versus a closed-form computation, with failures
/// injected at every chosen step and candidate effects never dispatched on replay.
pub fn correction(data: &[u8]) {
    scoped_correction(data);
    let mut fixture = Fixture::new(data);
    let old = fixture.live.read().0;
    let old_state = *fixture.live.read().1;
    let old_effects: Vec<_> = fixture
        .live
        .tentative_effects()
        .map(|effect| effect.descriptor)
        .collect();
    let count = fixture.values.len();
    let input_correction = byte(data, 2) & 1 != 0;
    let corrected = if input_correction {
        usize::from(byte(data, 3)) % count + 1
    } else {
        usize::from(byte(data, 3)) % (count + 1)
    };
    let predecessor = corrected - usize::from(input_correction);
    let changed = u64::from(byte(data, 4)).wrapping_mul(0x1111_1111_1111_1111);
    let mut selected = fixture.values.clone();
    let mut replacement = None;
    let baseline = if input_correction {
        let mut record = fixture
            .history
            .input_at(
                old.branch(),
                TimelineStep::new(u64::try_from(corrected).unwrap()),
            )
            .unwrap();
        record.revision = InputRevision::new(1);
        record.digest = changed.digest();
        fixture.history.import_input(record, changed).unwrap();
        replacement = Some(record);
        selected[corrected - 1] = changed;
        oracle(fixture.initial, &fixture.values[..predecessor])
    } else {
        changed
    };
    let base = anchor(old.branch(), u64::try_from(predecessor).unwrap(), baseline);
    if !input_correction {
        fixture.history.import_anchor(base, baseline).unwrap();
    }
    let mut scratch = [input(old.branch(), 0, 0); 32];
    let original = fixture
        .history
        .collect_inputs(old.branch(), base.step, old.step(), &mut scratch)
        .unwrap();
    let request = replacement.map_or(Correction::State(base), |record| Correction::Input {
        anchor: base,
        replacement: record,
    });
    let plan = ReplayPlan::prepare(old, old.revision(), request, original, LIMITS).unwrap();
    let observed = observation(
        TimelineStep::new(u64::try_from(corrected).unwrap()),
        replacement.map_or(base.digest, |record| record.digest),
        1,
    );
    let reservation = fixture.live.reserve(&plan, observed).unwrap();
    fixture.history.fork(&plan, reservation).unwrap();
    let length = plan.len();
    let failure = byte(data, 5) % 6;
    let failure_offset = usize::from(byte(data, 6)) % (length + 1);
    let budget = if failure == 1 {
        EffectLimits {
            max_intents: failure_offset,
            max_payload_bytes: 256,
        }
    } else if failure == 2 {
        EffectLimits {
            max_intents: 32,
            max_payload_bytes: u64::try_from(failure_offset * 8).unwrap(),
        }
    } else {
        FX
    };
    let failed_step = u64::try_from(predecessor + failure_offset + 1).unwrap();
    let model = Model {
        fail_at: (failure == 3).then_some(failed_step),
        duplicate_at: (failure == 4).then_some(failed_step),
    };
    let mut replay = fixture
        .history
        .begin_replay::<u64, 32>(plan, reservation, 0, budget)
        .unwrap();
    let mut failed = false;
    let mut completed = 0;
    for call in 0..=length + 1 {
        let allowance = if call == length + 1 {
            32
        } else {
            u64::from(byte(data, call + 16) % 4)
        };
        match replay.advance(old, &model, allowance) {
            Ok(phase) => {
                let previously = completed;
                completed = if phase == ReplayPhase::ReadyToValidate {
                    length
                } else {
                    (previously + usize::try_from(allowance).unwrap()).min(length)
                };
                assert_eq!(
                    *replay.scratch(),
                    oracle(baseline, &selected[predecessor..predecessor + completed])
                );
                if phase == ReplayPhase::ReadyToValidate {
                    break;
                }
            }
            Err(_error) => {
                failed = true;
                assert_eq!(replay.phase(), ReplayPhase::Aborted);
                break;
            }
        }
        assert_eq!(fixture.live.read(), (old, &old_state));
    }
    let should_fail = (1..=4).contains(&failure) && failure_offset < length;
    assert_eq!(failed, should_fail);
    if failed {
        assert!(replay.advance(old, &MODEL, 32).is_err());
        fixture.live.abort(reservation).unwrap();
        fixture
            .history
            .retire_branch(reservation.branch(), old)
            .unwrap();
        assert_eq!(fixture.live.read(), (old, &old_state));
        assert_eq!(
            fixture
                .live
                .tentative_effects()
                .map(|effect| effect.descriptor)
                .collect::<Vec<_>>(),
            old_effects
        );
        return;
    }
    let expected = oracle(baseline, &selected[predecessor..]);
    let checked = validator(if failure == 5 {
        expected.wrapping_add(1)
    } else {
        expected
    });
    let candidate = replay.validate(old, &checked);
    if failure == 5 {
        assert!(matches!(candidate, Err(TimelineError::ValidationFailed)));
        fixture.live.abort(reservation).unwrap();
        assert_eq!(fixture.live.read(), (old, &old_state));
        return;
    }
    let new = fixture.live.promote(candidate.unwrap()).unwrap();
    assert_eq!(fixture.live.read(), (new, &expected));
    assert_eq!(fixture.live.eligible_effects().count(), 0);
    for effect in fixture.live.tentative_effects() {
        let step = usize::try_from(effect.descriptor.origin.step.get()).unwrap();
        assert_eq!(effect.payload, selected[step - 1]);
        if step < corrected {
            assert!(old_effects.contains(&effect.descriptor));
        } else {
            assert_eq!(effect.descriptor.origin.branch, new.branch());
            assert_eq!(effect.descriptor.origin.revision, new.revision());
        }
    }
    let expected_count = if input_correction {
        count
    } else {
        count - usize::from(corrected > 0)
    };
    assert_eq!(fixture.live.tentative_effects().count(), expected_count);
    assert_eq!(
        fixture
            .history
            .snapshot_at(old.branch(), old.step())
            .unwrap()
            .digest,
        old_state.digest()
    );
    fixture
        .history
        .insert_snapshot(anchor(new.branch(), new.step().get(), expected), expected)
        .unwrap();
    fixture
        .history
        .expire_before(TimelineStep::new(u64::try_from(predecessor).unwrap()))
        .unwrap();
    assert_eq!(
        fixture.history.floor(),
        TimelineStep::new(u64::try_from(predecessor).unwrap())
    );
    // Materialization preserves semantic input versions and drops unused imports.
    let detached = fixture.history.materialize_branch(new.branch()).unwrap();
    assert_eq!(detached.branch_count(), 1);
    assert_eq!(detached.last_branch(), fixture.history.last_branch());
    for (offset, value) in selected.iter().enumerate().skip(predecessor) {
        let record = detached
            .input_at(
                new.branch(),
                TimelineStep::new(u64::try_from(offset + 1).unwrap()),
            )
            .unwrap();
        assert_eq!(detached.read(record).unwrap().value, value);
    }
}

/// Competing candidates, live input/authority/finality invalidation and abort.
pub fn promotion(data: &[u8]) {
    let mut fixture = Fixture::new(data);
    let pinned = fixture.live.read().0;
    let original = *fixture.live.read().1;
    let baseline = u64::from(byte(data, 2));
    let corrected = anchor(pinned.branch(), 0, baseline);
    fixture.history.import_anchor(corrected, baseline).unwrap();
    let original_inputs = records(pinned.branch(), 0, &fixture.values);
    let first_plan = ReplayPlan::prepare(
        pinned,
        pinned.revision(),
        Correction::State(corrected),
        &original_inputs,
        LIMITS,
    )
    .unwrap();
    let second_plan = ReplayPlan::prepare(
        pinned,
        pinned.revision(),
        Correction::State(corrected),
        &original_inputs,
        LIMITS,
    )
    .unwrap();
    let observed = observation(corrected.step, corrected.digest, 1);
    let first = fixture.live.reserve(&first_plan, observed).unwrap();
    let second = fixture.live.reserve(&second_plan, observed).unwrap();
    fixture.history.fork(&first_plan, first).unwrap();
    fixture.history.fork(&second_plan, second).unwrap();
    let expected = oracle(baseline, &fixture.values);
    let ready = |plan, reservation| {
        let mut session =
            ReplaySession::<u64, u64, 32>::begin(plan, reservation, 0, &fixture.history, FX)
                .unwrap();
        session
            .advance(pinned, &MODEL, &fixture.history, 32)
            .unwrap();
        session.validate(pinned, &validator(expected)).unwrap()
    };
    let first_candidate = ready(first_plan, first);
    let second_candidate = ready(second_plan, second);
    match byte(data, 3) % 6 {
        0 => {
            fixture.live.promote(first_candidate).unwrap();
            assert_eq!(
                fixture.live.promote(second_candidate),
                Err(TimelineError::Conflict)
            );
            assert_eq!(*fixture.live.read().1, expected);
        }
        1 => {
            fixture.live.promote(second_candidate).unwrap();
            assert_eq!(
                fixture.live.promote(first_candidate),
                Err(TimelineError::Conflict)
            );
            assert_eq!(*fixture.live.read().1, expected);
        }
        2 => {
            let value = u64::from(byte(data, 4));
            let advanced = original.wrapping_mul(3).wrapping_add(value);
            append(&mut fixture.history, &mut fixture.live, value, advanced);
            assert_eq!(
                fixture.live.promote(first_candidate),
                Err(TimelineError::Conflict)
            );
            assert_eq!(
                fixture.live.promote(second_candidate),
                Err(TimelineError::Conflict)
            );
            assert_eq!(*fixture.live.read().1, advanced);
            assert_eq!(fixture.live.read().0.step().get(), pinned.step().get() + 1);
        }
        3 | 4 => {
            if byte(data, 3) % 6 == 3 {
                fixture
                    .live
                    .observe(pinned, observation(pinned.step(), original.digest(), 2))
                    .unwrap();
            } else {
                fixture
                    .live
                    .finalize_effects(pinned, TimelineStep::ZERO)
                    .unwrap();
            }
            assert_eq!(
                fixture.live.promote(first_candidate),
                Err(TimelineError::Conflict)
            );
            assert_eq!(
                fixture.live.promote(second_candidate),
                Err(TimelineError::Conflict)
            );
            assert_eq!(*fixture.live.read().1, original);
        }
        _ => {
            fixture.live.abort(first).unwrap();
            assert_eq!(
                fixture.live.promote(first_candidate),
                Err(TimelineError::SessionUnavailable)
            );
            fixture.live.promote(second_candidate).unwrap();
            assert_eq!(*fixture.live.read().1, expected);
        }
    }
}

struct Durable {
    stored: Vec<(EffectDescriptor, u64)>,
    called: Vec<EffectDescriptor>,
    fail_at: usize,
    mode: u8,
}
impl EffectHandoff<u64> for Durable {
    type Error = ();
    #[allow(clippy::panic_in_result_fn)]
    fn submit(&mut self, effect: EligibleEffect<'_, u64>) -> Result<EffectAdmissionReceipt, ()> {
        let descriptor = effect.descriptor();
        let call = self.called.len();
        self.called.push(descriptor);
        assert_eq!(effect.payload().digest(), descriptor.digest);
        if self.mode == 3 && call == self.fail_at {
            return Err(());
        }
        if let Some(old) = self
            .stored
            .iter()
            .find(|old| old.0.identity == descriptor.identity)
        {
            assert_eq!(old, &(descriptor, *effect.payload()));
        } else {
            self.stored.push((descriptor, *effect.payload()));
        }
        if self.mode == 1 && call == self.fail_at {
            return Err(());
        }
        let mut receipt = EffectAdmissionReceipt {
            descriptor,
            durable_commit: Digest([17; 32]),
        };
        if self.mode == 2 && call == self.fail_at {
            receipt.descriptor.digest = Digest([99; 32]);
        }
        Ok(receipt)
    }
}
/// Finality and bounded partial handoff versus an independent pending-intent map.
pub fn effects(data: &[u8]) {
    let mut fixture = Fixture::new(data);
    let mut pending: BTreeMap<_, _> = fixture
        .live
        .tentative_effects()
        .map(|effect| {
            (
                effect.descriptor.origin.step.get(),
                (effect.descriptor, effect.payload),
            )
        })
        .collect();
    let count = fixture.values.len() as u64;
    let mut frontier = None;
    let mut sequence = 0;
    let mut durable = Durable {
        stored: Vec::new(),
        called: Vec::new(),
        fail_at: 0,
        mode: 0,
    };
    for pair in data.chunks(2).take(64) {
        let parameter = byte(pair, 1);
        let current = fixture.live.read().0;
        match byte(pair, 0) % 4 {
            0 => {
                let through = u64::from(parameter) % (count + 2);
                let valid = through <= count && frontier.is_none_or(|old| through > old);
                assert_eq!(
                    fixture
                        .live
                        .finalize_effects(current, TimelineStep::new(through))
                        .is_ok(),
                    valid
                );
                if valid {
                    frontier = Some(through);
                }
            }
            1 => {
                let eligible: Vec<_> = pending
                    .iter()
                    .filter(|(step, _)| frontier.is_some_and(|last| **step <= last))
                    .map(|(step, (descriptor, _value))| (*step, *descriptor))
                    .collect();
                let max = usize::from(parameter % 5);
                durable.called.clear();
                durable.fail_at = usize::from((parameter / 5) % 5);
                durable.mode = parameter / 32 % 4;
                let requested = max.min(eligible.len());
                let fails = durable.mode != 0 && durable.fail_at < requested;
                let acknowledged = if fails { durable.fail_at } else { requested };
                let result = fixture.live.handoff(&mut durable, max);
                if fails {
                    assert!(result.is_err());
                } else {
                    assert_eq!(result.unwrap(), acknowledged);
                }
                let calls = acknowledged + usize::from(fails);
                assert_eq!(
                    durable.called,
                    eligible
                        .iter()
                        .take(calls)
                        .map(|entry| entry.1)
                        .collect::<Vec<_>>()
                );
                for (step, _descriptor) in eligible.iter().take(acknowledged) {
                    pending.remove(step);
                }
            }
            2 => {
                sequence += 1;
                fixture
                    .live
                    .observe(
                        current,
                        observation(current.step(), fixture.live.read().1.digest(), sequence),
                    )
                    .unwrap();
            }
            _ => {
                let correction = anchor(current.branch(), 0, 0);
                let inputs = records(current.branch(), 0, &fixture.values);
                let plan = ReplayPlan::prepare(
                    current,
                    current.revision(),
                    Correction::State(correction),
                    &inputs,
                    LIMITS,
                );
                assert_eq!(plan.is_ok(), frontier.is_none());
            }
        }
        assert_eq!(
            fixture
                .live
                .read()
                .0
                .effect_finalized()
                .map(TimelineStep::get),
            frontier
        );
        let actual: BTreeMap<_, _> = fixture
            .live
            .tentative_effects()
            .map(|effect| {
                (
                    effect.descriptor.origin.step.get(),
                    (effect.descriptor, effect.payload),
                )
            })
            .collect();
        assert_eq!(actual, pending);
        assert_eq!(
            fixture.live.eligible_effects().count(),
            pending
                .keys()
                .filter(|step| frontier.is_some_and(|last| **step <= last))
                .count()
        );
        assert_eq!(
            *fixture.live.read().1,
            oracle(fixture.initial, &fixture.values)
        );
    }
}

// Exercise the convenience lifecycle against the same independent closed form.
// This is called by the correction property and its instrumented fuzz target.
fn scoped_correction(data: &[u8]) {
    let count = usize::from(byte(data, 0) % 16) + 1;
    let values: Vec<u64> = (0..count)
        .map(|offset| u64::from(byte(data, offset + 8)))
        .collect();
    let initial = u64::from(byte(data, 1));
    let mut live = LiveTimeline::<u64, u64, 1, 32>::new(head(0), initial, FX);
    let mut captured = Vec::new();
    for (offset, value) in values.iter().enumerate() {
        let expected = oracle(initial, &values[..=offset]);
        let record = live
            .apply_input(
                &MODEL,
                TimelineInputId::from_digest(Digest::of(&offset.to_le_bytes())),
                value,
                &validator(expected),
            )
            .unwrap();
        captured.push(CapturedInput::new(record, *value).unwrap());
        assert_eq!(*live.read().1, expected);
    }
    struct Inputs(Vec<CapturedInput<u64>>);
    impl InputStore<u64> for Inputs {
        type Error = TimelineError;
        fn read(&self, record: InputRecord) -> Result<BoundInput<'_, u64>, TimelineError> {
            self.0.as_slice().read(record)
        }
    }
    let inputs = Inputs(captured);
    let old_head = live.read().0;
    let old_state = *live.read().1;
    let corrected_at = usize::from(byte(data, 3)) % (count + 1);
    let corrected = u64::from(byte(data, 4));
    let snapshot = CapturedSnapshot::capture(
        old_head,
        TimelineStep::new(u64::try_from(corrected_at).unwrap()),
        TimelineSnapshotId::from_digest(corrected.digest()),
        corrected,
    )
    .unwrap();
    let records: Vec<_> = inputs
        .0
        .iter()
        .skip(corrected_at)
        .map(CapturedInput::record)
        .collect();
    let request = || CorrectionRequest {
        correction: Correction::State(snapshot.anchor()),
        inputs: &records,
        observation: observation(snapshot.anchor().step, snapshot.anchor().digest, 1),
        scratch: 0,
        limits: LIMITS,
    };
    let expected = oracle(corrected, &values[corrected_at..]);
    {
        let mut attempt = live.begin_correction(request(), &snapshot).unwrap();
        match byte(data, 5) % 3 {
            0 => {
                drop(attempt);
            }
            1 => {
                attempt.advance(&MODEL, &inputs, 32).unwrap();
                assert_eq!(
                    attempt.publish(&validator(expected.wrapping_add(1))),
                    Err(TimelineError::ValidationFailed)
                );
            }
            2 => {
                let failing = Model {
                    fail_at: Some(u64::try_from(corrected_at + 1).unwrap()),
                    duplicate_at: None,
                };
                if corrected_at < count {
                    assert!(attempt.advance(&failing, &inputs, 32).is_err());
                    assert_eq!(
                        attempt.publish(&validator(expected)),
                        Err(TimelineError::InvalidPhase)
                    );
                }
            }
            _ => {}
        }
    }
    assert_eq!(live.read(), (old_head, &old_state));
    // With only one slot, this also checks cleanup after every failed/cancelled job.
    let mut correction = live.begin_correction(request(), &snapshot).unwrap();
    let _phase = correction.advance(&MODEL, &inputs, 0).unwrap();
    let budget = u64::from(byte(data, 7) % 4) + 1;
    while correction.advance(&MODEL, &inputs, budget).unwrap() == ReplayPhase::Replaying {}
    let selected = correction
        .publish(&|state: &u64, proposed: TimelineHead| {
            assert_eq!(proposed.scope(), old_head.scope());
            assert_eq!(proposed.step(), old_head.step());
            assert_eq!(
                proposed.revision(),
                old_head.revision().checked_next().unwrap()
            );
            assert!(proposed.branch() > old_head.branch());
            assert_eq!(
                proposed.authority(),
                Some(observation(
                    snapshot.anchor().step,
                    snapshot.anchor().digest,
                    1
                ))
            );
            assert_eq!(proposed.effect_finalized(), old_head.effect_finalized());
            validator(expected).validate(state, proposed)
        })
        .unwrap();
    assert_eq!(*live.read().1, expected);
    assert_eq!(selected.step(), old_head.step());
    assert_eq!(
        selected.revision(),
        old_head.revision().checked_next().unwrap()
    );
    assert!(selected.branch() > old_head.branch());
    assert_eq!(selected.effect_finalized(), None);
}
