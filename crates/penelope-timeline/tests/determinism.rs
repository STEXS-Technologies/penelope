//! Conditional determinism: identical captured baselines versus omitted dependencies.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::cell::Cell;
const FX: EffectLimits = EffectLimits {
    max_intents: 0,
    max_payload_bytes: 0,
};
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
        TimelineBranchId::ZERO,
        TimelineStep::new(2),
        TimelineRevision::ZERO,
        None,
    )
    .unwrap()
}
fn anchor<T: HistoryPayload>(state: &T) -> StateAnchor {
    StateAnchor {
        scope: scope(),
        branch: TimelineBranchId::ZERO,
        step: TimelineStep::ZERO,
        snapshot: TimelineSnapshotId::from_digest(state.digest()),
        digest: state.digest(),
    }
}
fn input(step: u64, value: u64) -> InputRecord {
    InputRecord {
        scope: scope(),
        branch: TimelineBranchId::ZERO,
        step: TimelineStep::new(step),
        identity: TimelineInputId::from_digest(Digest::of(&step.to_le_bytes())),
        revision: InputRevision::ZERO,
        digest: value.digest(),
    }
}
struct Inputs;
impl InputStore<u64> for Inputs {
    type Error = TimelineError;
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, u64>, TimelineError> {
        static VALUES: [u64; 2] = [1, 2];
        let index = record
            .step
            .get()
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or(TimelineError::InputMismatch)?;
        let value = VALUES.get(index).ok_or(TimelineError::InputMismatch)?;
        if record != input(record.step.get(), *value) {
            return Err(TimelineError::InputMismatch);
        }
        Ok(BoundInput { record, value })
    }
}
struct Snapshot<T> {
    state: T,
    anchor: StateAnchor,
}
impl<T: Copy + HistoryPayload> SnapshotStore<T> for Snapshot<T> {
    type Error = TimelineError;
    fn restore(
        &self,
        requested: StateAnchor,
        scratch: &mut T,
    ) -> Result<StateAnchor, TimelineError> {
        if requested != self.anchor || self.state.digest() != requested.digest {
            return Err(TimelineError::InputMismatch);
        }
        *scratch = self.state;
        Ok(requested)
    }
}
// Setup errors must panic so tests cannot confuse them with the intended validation refusal.
#[allow(clippy::unwrap_in_result)]
fn replay<T: Copy + HistoryPayload, M, S>(
    live: &mut LiveTimeline<T, (), 3, 0>,
    model: &M,
    snapshots: &S,
    baseline: StateAnchor,
    expected: Digest,
) -> Result<ReadyCandidate<T, (), 0>, TimelineError>
where
    M: RewindableComputation<State = T, Input = u64, Effect = (), Error = ()>,
    S: SnapshotStore<T, Error = TimelineError>,
{
    let current = live.read().0;
    let inputs = [input(1, 1), input(2, 2)];
    let plan = ReplayPlan::prepare(
        current,
        current.revision(),
        Correction::State(baseline),
        &inputs,
        ReplayLimits {
            max_retained_inputs: 2,
            max_replay_steps: 2,
        },
    )
    .unwrap();
    let reservation = live
        .reserve(
            &plan,
            AuthorityObservation {
                scope: scope(),
                authority: TimelineAuthorityId::from_digest(Digest([5; 32])),
                epoch: AuthorityEpoch::ZERO,
                sequence: AuthoritySequence::new(1),
                step: baseline.step,
                digest: baseline.digest,
            },
        )
        .unwrap();
    let mut session =
        ReplaySession::<T, (), 0>::begin(plan, reservation, *live.read().1, snapshots, FX).unwrap();
    session.advance(current, model, &Inputs, 2).unwrap();
    session.validate(
        current,
        &DigestValidator {
            expected,
            digest: T::digest,
        },
    )
}
struct Ambient {
    counter: Cell<u64>,
}
impl RewindableComputation for Ambient {
    type State = u64;
    type Input = u64;
    type Effect = ();
    type Error = ();
    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut u64,
        input: &u64,
        _context: StepContext,
        _sink: &mut S,
    ) -> Result<(), ()> {
        *state += *input + self.counter.get();
        self.counter.set(self.counter.get() + 1);
        Ok(())
    }
}
#[test]
fn uncaptured_dependency_is_detected_only_when_same_baseline_verification_is_enabled() {
    let baseline = 0_u64;
    let snapshots = Snapshot {
        state: baseline,
        anchor: anchor(&baseline),
    };
    let mut live = LiveTimeline::<u64, (), 3, 0>::new(head(), 99, FX);
    let model = Ambient {
        counter: Cell::new(0),
    };
    let first = replay(
        &mut live,
        &model,
        &snapshots,
        snapshots.anchor,
        4_u64.digest(),
    );
    assert!(first.is_ok()); // 0 + (1+0) + (2+1).
    let second = replay(
        &mut live,
        &model,
        &snapshots,
        snapshots.anchor,
        4_u64.digest(),
    );
    assert!(matches!(second, Err(TimelineError::ValidationFailed))); // Hidden counter is now 2.
    assert_eq!(live.read(), (head(), &99));
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pair {
    left: u64,
    right: u64,
}
impl HistoryPayload for Pair {
    fn digest(&self) -> Digest {
        let mut bytes = [0; 16];
        let (left, right) = bytes.split_at_mut(8);
        left.copy_from_slice(&self.left.to_le_bytes());
        right.copy_from_slice(&self.right.to_le_bytes());
        Digest::of(&bytes)
    }
    fn byte_len(&self) -> u64 {
        16
    }
}
struct Captured;
impl RewindableComputation for Captured {
    type State = Pair;
    type Input = u64;
    type Effect = ();
    type Error = ();
    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut Pair,
        input: &u64,
        _context: StepContext,
        _sink: &mut S,
    ) -> Result<(), ()> {
        state.left += *input + state.right;
        state.right += 1;
        Ok(())
    }
}
#[test]
fn captured_dependency_replays_identically_from_the_same_complete_state() {
    let baseline = Pair { left: 0, right: 0 };
    let snapshots = Snapshot {
        state: baseline,
        anchor: anchor(&baseline),
    };
    let current = Pair { left: 99, right: 7 };
    let mut live = LiveTimeline::<Pair, (), 3, 0>::new(head(), current, FX);
    let expected = Pair { left: 4, right: 2 }.digest();
    assert!(replay(&mut live, &Captured, &snapshots, snapshots.anchor, expected).is_ok());
    assert!(replay(&mut live, &Captured, &snapshots, snapshots.anchor, expected).is_ok());
    assert_eq!(live.read(), (head(), &current));
}
struct IncompleteSnapshot {
    state: Pair,
    anchor: StateAnchor,
}
impl SnapshotStore<Pair> for IncompleteSnapshot {
    type Error = TimelineError;
    fn restore(
        &self,
        requested: StateAnchor,
        scratch: &mut Pair,
    ) -> Result<StateAnchor, TimelineError> {
        if requested != self.anchor {
            return Err(TimelineError::InputMismatch);
        }
        // Deliberately broken consumer adapter: silently omit a read dependency.
        *scratch = Pair {
            left: self.state.left,
            right: 0,
        };
        Ok(requested)
    }
}
#[test]
fn full_domain_validation_refuses_a_snapshot_that_omits_a_dependency() {
    let baseline = Pair { left: 1, right: 7 };
    let snapshots = IncompleteSnapshot {
        state: baseline,
        anchor: anchor(&baseline),
    };
    let current = Pair { left: 99, right: 7 };
    let mut live = LiveTimeline::<Pair, (), 3, 0>::new(head(), current, FX);
    let expected = Pair { left: 19, right: 9 }.digest(); // 1+(1+7)+(2+8), counter 7->9.
    assert!(matches!(
        replay(&mut live, &Captured, &snapshots, snapshots.anchor, expected),
        Err(TimelineError::ValidationFailed)
    ));
    assert_eq!(live.read(), (head(), &current));
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Floating {
    value: f64,
}
impl HistoryPayload for Floating {
    fn digest(&self) -> Digest {
        Digest::of(&self.value.to_bits().to_le_bytes())
    }
    fn byte_len(&self) -> u64 {
        8
    }
}
struct Reduction {
    regroup: Cell<bool>,
}
// Floating-point nonassociativity is the deliberate adversarial input in this test.
#[allow(clippy::float_arithmetic)]
impl RewindableComputation for Reduction {
    type State = Floating;
    type Input = u64;
    type Effect = ();
    type Error = ();
    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut Floating,
        _input: &u64,
        _context: StepContext,
        _sink: &mut S,
    ) -> Result<(), ()> {
        let large = std::hint::black_box(1.0e16);
        let small = std::hint::black_box(1.0);
        state.value += if self.regroup.get() {
            (large + -large) + small
        } else {
            large + (-large + small)
        };
        Ok(())
    }
}
#[test]
fn changed_floating_point_reduction_order_fails_identical_replay_verification() {
    let baseline = Floating { value: 0.0 };
    let snapshots = Snapshot {
        state: baseline,
        anchor: anchor(&baseline),
    };
    let current = Floating { value: 99.0 };
    let mut live = LiveTimeline::<Floating, (), 3, 0>::new(head(), current, FX);
    let model = Reduction {
        regroup: Cell::new(false),
    };
    assert!(
        replay(
            &mut live,
            &model,
            &snapshots,
            snapshots.anchor,
            baseline.digest()
        )
        .is_ok()
    );
    model.regroup.set(true); // Uncaptured execution configuration, like reordered parallel reduction.
    assert!(matches!(
        replay(
            &mut live,
            &model,
            &snapshots,
            snapshots.anchor,
            baseline.digest()
        ),
        Err(TimelineError::ValidationFailed)
    ));
    assert_eq!(live.read(), (head(), &current));
}
