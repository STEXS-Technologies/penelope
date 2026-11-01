//! Integer-only simulation: late state/input correction, isolated replay and retention.
use penelope_core::task::Digest;
use penelope_timeline::*;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct World {
    position: i64,
    velocity: i64,
}
#[derive(Clone, Copy)]
struct Movement {
    acceleration: i64,
    wind: i64,
}
impl HistoryPayload for World {
    fn digest(&self) -> Digest {
        let mut bytes = [0; 16];
        let (position, velocity) = bytes.split_at_mut(8);
        position.copy_from_slice(&self.position.to_le_bytes());
        velocity.copy_from_slice(&self.velocity.to_le_bytes());
        Digest::of(&bytes)
    }
    fn byte_len(&self) -> u64 {
        16
    }
}
impl HistoryPayload for Movement {
    fn digest(&self) -> Digest {
        World {
            position: self.acceleration,
            velocity: self.wind,
        }
        .digest()
    }
    fn byte_len(&self) -> u64 {
        16
    }
}
struct Simulation;
impl RewindableComputation for Simulation {
    type State = World;
    type Input = Movement;
    type Effect = ();
    type Error = TimelineError;
    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut World,
        input: &Movement,
        _context: StepContext,
        _effects: &mut S,
    ) -> Result<(), TimelineError> {
        state.velocity = state
            .velocity
            .checked_add(input.acceleration)
            .ok_or(TimelineError::ValidationFailed)?;
        state.position = state
            .position
            .checked_add(state.velocity)
            .and_then(|position| position.checked_add(input.wind))
            .ok_or(TimelineError::ValidationFailed)?;
        Ok(())
    }
}
// Independent closed-form integer oracle. Wind is captured in each input; there
// is no ambient RNG/time or engine state outside the complete World snapshot.
fn reference(baseline: World, inputs: &[Movement]) -> Result<World, TimelineError> {
    let mut displacement = 0_i128;
    let mut acceleration = 0_i128;
    let count = i128::try_from(inputs.len()).map_err(|_error| TimelineError::Capacity)?;
    for (offset, input) in inputs.iter().enumerate() {
        let offset = i128::try_from(offset).map_err(|_error| TimelineError::Capacity)?;
        let remaining = count
            .checked_sub(offset)
            .ok_or(TimelineError::CounterExhausted)?;
        displacement = displacement
            .checked_add(
                i128::from(input.acceleration)
                    .checked_mul(remaining)
                    .ok_or(TimelineError::ValidationFailed)?,
            )
            .and_then(|value| value.checked_add(i128::from(input.wind)))
            .ok_or(TimelineError::ValidationFailed)?;
        acceleration = acceleration
            .checked_add(i128::from(input.acceleration))
            .ok_or(TimelineError::ValidationFailed)?;
    }
    let position = i128::from(baseline.position)
        .checked_add(
            i128::from(baseline.velocity)
                .checked_mul(count)
                .ok_or(TimelineError::ValidationFailed)?,
        )
        .and_then(|value| value.checked_add(displacement))
        .ok_or(TimelineError::ValidationFailed)?;
    Ok(World {
        position: i64::try_from(position).map_err(|_error| TimelineError::ValidationFailed)?,
        velocity: i64::try_from(
            i128::from(baseline.velocity)
                .checked_add(acceleration)
                .ok_or(TimelineError::ValidationFailed)?,
        )
        .map_err(|_error| TimelineError::ValidationFailed)?,
    })
}
const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
fn anchor(branch: TimelineBranchId, step: TimelineStep, world: World) -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch,
        step,
        snapshot: TimelineSnapshotId::from_digest(world.digest()),
        digest: world.digest(),
    }
}
fn record(branch: TimelineBranchId, step: TimelineStep, movement: Movement) -> InputRecord {
    InputRecord {
        scope: scope(),
        branch,
        step,
        identity: TimelineInputId::from_digest(Digest::of(&step.get().to_le_bytes())),
        revision: InputRevision::ZERO,
        digest: movement.digest(),
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
fn validator(expected: World) -> DigestValidator<fn(&World) -> Digest> {
    DigestValidator {
        expected: expected.digest(),
        digest: World::digest,
    }
}
const FX: EffectLimits = EffectLimits {
    max_intents: 0,
    max_payload_bytes: 0,
};
const LIMITS: ReplayLimits = ReplayLimits {
    max_retained_inputs: 16,
    max_replay_steps: 16,
};
type History = FixedHistory<World, Movement, 16, 24, 4>;
#[derive(Debug, Error)]
enum ExampleError {
    #[error(transparent)]
    Timeline(#[from] TimelineError),
    #[error(transparent)]
    Replay(#[from] ReplayError<TimelineError, TimelineError>),
    #[error(transparent)]
    Restore(#[from] RestoreError<TimelineError>),
    #[error(transparent)]
    Step(#[from] StepError<TimelineError, TimelineError, TimelineError>),
}
// Assertions make this runnable acceptance example fail visibly on a contract regression.
#[allow(clippy::panic_in_result_fn)]
fn scenario() -> Result<(), ExampleError> {
    let initial = World::default();
    let genesis = TimelineHead::new(
        scope(),
        TimelineBranchId::ZERO,
        TimelineStep::ZERO,
        TimelineRevision::ZERO,
        None,
    )?;
    let mut history = History::new(
        anchor(genesis.branch(), genesis.step(), initial),
        initial,
        HistoryLimits {
            max_input_bytes: 16,
            max_snapshot_bytes: 16,
            max_total_bytes: 4096,
        },
    )?;
    let mut live = LiveTimeline::<World, (), 2, 0>::new(genesis, initial, FX);
    let movements = [
        Movement {
            acceleration: 1,
            wind: 2,
        },
        Movement {
            acceleration: 2,
            wind: -1,
        },
        Movement {
            acceleration: -1,
            wind: 0,
        },
        Movement {
            acceleration: 3,
            wind: 1,
        },
        Movement {
            acceleration: 0,
            wind: -2,
        },
        Movement {
            acceleration: -2,
            wind: 1,
        },
    ];
    for (offset, movement) in movements.iter().copied().enumerate() {
        let current = live.read().0;
        let next = current
            .step()
            .checked_next()
            .ok_or(TimelineError::CounterExhausted)?;
        let input = record(current.branch(), next, movement);
        history.append_input(input, movement)?;
        let through = offset.checked_add(1).ok_or(TimelineError::Capacity)?;
        let expected = reference(
            initial,
            movements.get(..through).ok_or(TimelineError::Capacity)?,
        )?;
        let ready = prepare_live_step::<_, _, _, _, 0>(
            LiveStepRequest {
                expected: current,
                anchor: history.snapshot_at(current.branch(), current.step())?,
                input,
                scratch: initial,
                limits: FX,
            },
            &Simulation,
            &history,
            &history,
            &validator(expected),
        )?;
        let advanced = live.append(ready)?;
        history.capture(
            anchor(advanced.branch(), advanced.step(), *live.read().1),
            live.read().1,
        )?;
    }
    let original = live.read().0;
    let original_state = *live.read().1;
    assert_eq!(original_state, reference(initial, &movements)?);
    // The caller authenticated this complete post-step S2 before importing it.
    let corrected_state = World {
        position: 100,
        velocity: 4,
    };
    let corrected = anchor(original.branch(), TimelineStep::new(2), corrected_state);
    history.import_anchor(corrected, corrected_state)?;
    let filler = record(original.branch(), TimelineStep::ZERO, movements[0]);
    let mut scratch = [filler; 16];
    let inputs = history.collect_inputs(
        original.branch(),
        corrected.step,
        original.step(),
        &mut scratch,
    )?;
    let plan = ReplayPlan::prepare(
        original,
        original.revision(),
        Correction::State(corrected),
        inputs,
        LIMITS,
    )?;
    let reservation = live.reserve(&plan, observation(corrected.step, corrected.digest, 1))?;
    history.fork(&plan, reservation)?;
    let mut replay = history.begin_replay::<(), 0>(plan, reservation, initial, FX)?;
    let mut captured = [None; 4];
    let mut step = corrected.step;
    for slot in &mut captured {
        replay.advance(original, &Simulation, 1)?;
        step = step.checked_next().ok_or(TimelineError::CounterExhausted)?;
        *slot = Some((step, *replay.scratch()));
        assert_eq!(live.read(), (original, &original_state));
    }
    let suffix = movements.get(2..).ok_or(TimelineError::Capacity)?;
    let expected = reference(corrected_state, suffix)?;
    let ready = replay.validate(original, &validator(expected))?;
    for (captured_step, state) in captured.into_iter().flatten() {
        history.insert_snapshot(anchor(reservation.branch(), captured_step, state), state)?;
    }
    let first = live.promote(ready)?;
    assert_eq!(*live.read().1, expected);
    // Replace only I4, restoring the child's complete S3, then replay I4..I6.
    let predecessor = history.snapshot_at(first.branch(), TimelineStep::new(3))?;
    let mut baseline = initial;
    history.restore(predecessor, &mut baseline)?;
    let replacement_value = Movement {
        acceleration: -8,
        wind: 3,
    };
    let mut replacement = history.input_at(first.branch(), TimelineStep::new(4))?;
    replacement.revision = InputRevision::new(1);
    replacement.digest = replacement_value.digest();
    history.import_input(replacement, replacement_value)?;
    let replacement_inputs =
        history.collect_inputs(first.branch(), predecessor.step, first.step(), &mut scratch)?;
    let replacement_plan = ReplayPlan::prepare(
        first,
        first.revision(),
        Correction::Input {
            anchor: predecessor,
            replacement,
        },
        replacement_inputs,
        LIMITS,
    )?;
    let replacement_reservation = live.reserve(
        &replacement_plan,
        observation(replacement.step, replacement.digest, 2),
    )?;
    history.fork(&replacement_plan, replacement_reservation)?;
    let mut replacement_replay =
        history.begin_replay::<(), 0>(replacement_plan, replacement_reservation, initial, FX)?;
    assert_eq!(
        replacement_replay.advance(first, &Simulation, 0)?,
        ReplayPhase::Replaying
    );
    replacement_replay.advance(first, &Simulation, 1)?;
    replacement_replay.advance(first, &Simulation, 2)?;
    let replacement_expected =
        reference(baseline, &[replacement_value, movements[4], movements[5]])?;
    let replacement_ready = replacement_replay.validate(first, &validator(replacement_expected))?;
    let current = live.promote(replacement_ready)?;
    history.capture(
        anchor(current.branch(), current.step(), *live.read().1),
        live.read().1,
    )?;
    assert_eq!(*live.read().1, replacement_expected);
    assert_eq!(
        history
            .snapshot_at(original.branch(), original.step())?
            .digest,
        original_state.digest()
    );
    history.expire_before(TimelineStep::new(3))?;
    history = history.materialize_branch(current.branch())?;
    assert_eq!(history.branch_count(), 1);
    assert_eq!(
        history
            .input_at(current.branch(), replacement.step)?
            .revision,
        InputRevision::new(1)
    );
    println!(
        "simulation: step={} branch={} position={} velocity={} retained_inputs={}",
        current.step().get(),
        current.branch().get(),
        replacement_expected.position,
        replacement_expected.velocity,
        history.input_count()
    );
    Ok(())
}
fn main() -> Result<(), ExampleError> {
    scenario()
}
#[test]
fn late_state_and_input_corrections_match_the_closed_form_oracle() -> Result<(), ExampleError> {
    scenario()
}
