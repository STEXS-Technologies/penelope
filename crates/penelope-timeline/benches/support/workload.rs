//! Same captured integer model for a handwritten ring and public timeline APIs.
//! Fixture construction and reporting are outside measured regions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![allow(
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::missing_const_for_fn
)]
#![allow(dead_code)] // Each executable uses only one reporter.
use penelope_core::task::Digest;
use penelope_timeline::*;
use std::{hint::black_box, time::Instant};

const WINDOW: usize = 512;
const FX: EffectLimits = EffectLimits {
    max_intents: 0,
    max_payload_bytes: 0,
};
const LIMITS: ReplayLimits = ReplayLimits {
    max_retained_inputs: 512,
    max_replay_steps: 512,
};
type Live<const W: usize> = LiveTimeline<State<W>, (), 2, 0>;
type History<const W: usize> = FixedHistory<State<W>, u64, 512, 65, 2>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct State<const W: usize>([u64; W]);
impl<const W: usize> HistoryPayload for State<W> {
    fn digest(&self) -> Digest {
        // No allocating serialization; this consumer explicitly enables snapshot
        // integrity hashes. Replay itself does not hash full state on each step.
        let mut bytes = [0_u8; 4096];
        for (index, word) in self.0.iter().enumerate() {
            bytes[index * 8..index * 8 + 8].copy_from_slice(&word.to_le_bytes());
        }
        Digest::of(&bytes[..W * 8])
    }
    fn byte_len(&self) -> u64 {
        u64::try_from(W * 8).unwrap()
    }
}
fn compute<const W: usize>(state: &mut State<W>, value: u64) {
    for (index, word) in state.0.iter_mut().enumerate() {
        *word = word
            .wrapping_mul(3)
            .wrapping_add(value)
            .wrapping_add(index as u64);
    }
}
impl<const W: usize> RewindableComputation for Computation<W> {
    type State = State<W>;
    type Input = u64;
    type Effect = ();
    type Error = TimelineError;
    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut State<W>,
        input: &u64,
        _context: StepContext,
        _effects: &mut S,
    ) -> Result<(), TimelineError> {
        compute(state, *input);
        Ok(())
    }
}
struct Computation<const W: usize>;
const fn scope() -> TimelineScope {
    TimelineScope {
        tenant: TimelineTenantId::from_digest(Digest([1; 32])),
        timeline: TimelineId::from_digest(Digest([2; 32])),
        model: TimelineModelId::from_digest(Digest([3; 32])),
        schema: TimelineSchemaId::from_digest(Digest([4; 32])),
    }
}
fn head(steps: usize) -> TimelineHead {
    TimelineHead::new(
        scope(),
        TimelineBranchId::ZERO,
        TimelineStep::new(steps as u64),
        TimelineRevision::new(steps as u64),
        None,
    )
    .unwrap()
}
fn anchor<const W: usize>(step: usize, state: State<W>) -> StateAnchor {
    let digest = state.digest();
    StateAnchor {
        scope: scope(),
        branch: TimelineBranchId::ZERO,
        step: TimelineStep::new(step as u64),
        snapshot: TimelineSnapshotId::from_digest(digest),
        digest,
    }
}
fn observation(anchor: StateAnchor) -> AuthorityObservation {
    AuthorityObservation {
        scope: scope(),
        authority: TimelineAuthorityId::from_digest(Digest([5; 32])),
        epoch: AuthorityEpoch::ZERO,
        sequence: AuthoritySequence::new(1),
        step: anchor.step,
        digest: anchor.digest,
    }
}
struct Fixture<const W: usize> {
    state: State<W>,
    anchor: StateAnchor,
    records: [InputRecord; WINDOW],
    values: [u64; WINDOW],
}
impl<const W: usize> Fixture<W> {
    fn new() -> Self {
        let state = State(core::array::from_fn(|i| i as u64 + 11));
        let values = core::array::from_fn(|i| (i as u64).wrapping_mul(17).wrapping_add(9));
        let records = core::array::from_fn(|i| InputRecord {
            scope: scope(),
            branch: TimelineBranchId::ZERO,
            step: TimelineStep::new(i as u64 + 1),
            identity: TimelineInputId::from_digest(Digest::of(&(i as u64).to_le_bytes())),
            revision: InputRevision::ZERO,
            digest: values[i].digest(),
        });
        Self {
            state,
            anchor: anchor(0, state),
            records,
            values,
        }
    }
    fn plan(&self, n: usize) -> ReplayPlan<'_> {
        ReplayPlan::prepare(
            head(n),
            head(n).revision(),
            Correction::State(self.anchor),
            &self.records[..n],
            LIMITS,
        )
        .unwrap()
    }
    fn live(&self, n: usize) -> Live<W> {
        Live::new(head(n), State([0; W]), FX)
    }
    fn session(&self, n: usize) -> (Live<W>, ReplaySession<'_, State<W>, (), 0>) {
        let mut live = self.live(n);
        let plan = self.plan(n);
        let reservation = live.reserve(&plan, observation(self.anchor)).unwrap();
        let session = ReplaySession::begin(plan, reservation, State([0; W]), self, FX).unwrap();
        (live, session)
    }
    fn expected(&self, n: usize) -> State<W> {
        // Closed form independent of compute's iterative state updates.
        State(core::array::from_fn(|lane| {
            let mut factor = 1_u64;
            let mut sum = 0_u64;
            for value in self.values[..n].iter().rev() {
                sum = sum.wrapping_add(value.wrapping_add(lane as u64).wrapping_mul(factor));
                factor = factor.wrapping_mul(3);
            }
            self.state.0[lane].wrapping_mul(factor).wrapping_add(sum)
        }))
    }
    fn history(&self, n: usize) -> History<W> {
        let mut store = History::new(
            self.anchor,
            self.state,
            HistoryLimits {
                max_input_bytes: 8,
                max_snapshot_bytes: (W * 8) as u64,
                max_total_bytes: 1_000_000,
            },
        )
        .unwrap();
        for i in 0..n {
            store.append_input(self.records[i], self.values[i]).unwrap();
        }
        store
    }
}
impl<const W: usize> SnapshotStore<State<W>> for Fixture<W> {
    type Error = TimelineError;
    fn restore(
        &self,
        anchor: StateAnchor,
        into: &mut State<W>,
    ) -> Result<StateAnchor, TimelineError> {
        if anchor != self.anchor {
            return Err(TimelineError::HistoryUnavailable);
        }
        *into = self.state;
        Ok(anchor)
    }
}
impl<const W: usize> InputStore<u64> for Fixture<W> {
    type Error = TimelineError;
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, u64>, TimelineError> {
        let offset = record
            .step
            .get()
            .checked_sub(1)
            .ok_or(TimelineError::InputMismatch)? as usize;
        if self.records.get(offset) != Some(&record) {
            return Err(TimelineError::InputMismatch);
        }
        // Immutable owned values and preverified descriptors are pinned together.
        Ok(BoundInput {
            record,
            value: &self.values[offset],
        })
    }
}
struct Validator<const W: usize>(State<W>);
impl<const W: usize> CandidateValidator<State<W>> for Validator<W> {
    fn validate(&self, state: &State<W>, _expected: TimelineHead) -> Result<(), TimelineError> {
        if state == &self.0 {
            Ok(())
        } else {
            Err(TimelineError::ValidationFailed)
        }
    }
}

struct ReplacementInputs<'fixture, const W: usize> {
    fixture: &'fixture Fixture<W>,
    replacement: InputRecord,
    value: u64,
}
impl<const W: usize> InputStore<u64> for ReplacementInputs<'_, W> {
    type Error = TimelineError;
    fn read(&self, record: InputRecord) -> Result<BoundInput<'_, u64>, TimelineError> {
        if record == self.replacement {
            Ok(BoundInput {
                record,
                value: &self.value,
            })
        } else {
            self.fixture.read(record)
        }
    }
}

// Deliberately small reference: local numeric order and CAS only. It has no
// scope/authority/finality/intent/immutable-branch protocol. It is a lower-bound
// substrate comparison, not a replacement implementation of those guarantees.
struct Ring<const W: usize> {
    values: [u64; WINDOW],
    snapshots: [State<W>; 65],
    next: usize,
    revision: u64,
    live: State<W>,
}
impl<const W: usize> Ring<W> {
    fn new(fixture: &Fixture<W>, n: usize) -> Self {
        Self {
            values: fixture.values,
            snapshots: [fixture.state; 65],
            next: n % WINDOW,
            revision: n as u64,
            live: fixture.state,
        }
    }
    fn append(&mut self, value: u64) {
        self.values[self.next] = value;
        self.next = (self.next + 1) % WINDOW;
    }
    fn replay(&self, n: usize, budget: usize) -> State<W> {
        let mut scratch = self.snapshots[0];
        for chunk in self.values[..n].chunks(budget) {
            for input in chunk {
                compute(&mut scratch, *input);
            }
            black_box(&scratch);
        }
        scratch
    }
    fn promote(&mut self, expected: u64, state: State<W>) -> bool {
        if self.revision != expected {
            return false;
        }
        self.live = state;
        self.revision = self.revision.checked_add(1).unwrap();
        true
    }
}

pub trait Reporter {
    fn sample<T, R>(
        &mut self,
        name: &str,
        words: usize,
        steps: usize,
        setup: impl FnMut() -> T,
        operation: impl FnMut(&mut T) -> R,
    );
}
pub struct Latency {
    samples: usize,
}
impl Latency {
    pub fn new() -> Self {
        let samples =
            std::env::var("PENELOPE_TIMELINE_SAMPLES").map_or(200, |s| s.parse().unwrap());
        assert!(samples >= 100);
        Self { samples }
    }
}
impl Reporter for Latency {
    fn sample<T, R>(
        &mut self,
        name: &str,
        words: usize,
        steps: usize,
        mut setup: impl FnMut() -> T,
        mut operation: impl FnMut(&mut T) -> R,
    ) {
        let mut times = Vec::with_capacity(self.samples);
        for i in 0..self.samples + 16 {
            let mut input = black_box(setup());
            let start = Instant::now();
            let result = black_box(operation(black_box(&mut input)));
            let nanos = start.elapsed().as_nanos();
            black_box(result);
            if i >= 16 {
                times.push(nanos);
            }
        }
        times.sort_unstable();
        println!(
            "latency,{name},{words},{steps},{},{},{},{}",
            self.samples,
            times[self.samples / 2],
            times[(self.samples * 95).div_ceil(100) - 1],
            times[(self.samples * 99).div_ceil(100) - 1]
        );
    }
}
fn profiles<const W: usize>(report: &mut impl Reporter) {
    let fixture = Fixture::<W>::new();
    println!(
        "memory,{W},{},{},{},{},{}",
        std::mem::size_of::<History<W>>(),
        std::mem::size_of::<Live<W>>(),
        std::mem::size_of::<ReplaySession<'_, State<W>, (), 0>>(),
        std::mem::size_of::<Ring<W>>(),
        std::mem::size_of::<State<W>>()
    );
    for n in [1, 8, 32, 128, 512] {
        let expected = fixture.expected(n);
        let validator = Validator(expected);
        let (mut checked_live, mut checked_session) = fixture.session(n);
        checked_session
            .advance(head(n), &Computation::<W>, &fixture, n as u64)
            .unwrap();
        assert_eq!(*checked_session.scratch(), expected);
        checked_live
            .promote(checked_session.validate(head(n), &validator).unwrap())
            .unwrap();
        assert_eq!(*checked_live.read().1, expected);
        let mut reference_ring = Ring::new(&fixture, n);
        let reference_state = reference_ring.replay(n, 8);
        assert_eq!(reference_state, expected);
        assert!(reference_ring.promote(n as u64, reference_state));

        report.sample(
            "core_prepare",
            W,
            n,
            || (),
            |()| black_box(fixture.plan(black_box(n))),
        );
        report.sample(
            "core_reserve_promote",
            W,
            n,
            || TimelineProjection::<2>::new(head(n)),
            |projection| {
                let plan = fixture.plan(n);
                let token = projection
                    .reserve(&plan, observation(fixture.anchor))
                    .unwrap();
                projection.promote(token).unwrap()
            },
        );
        report.sample(
            "missing_window_refusal",
            W,
            n,
            || (),
            |()| {
                let result = ReplayPlan::prepare(
                    head(n),
                    head(n).revision(),
                    Correction::State(fixture.anchor),
                    &fixture.records[..n - 1],
                    LIMITS,
                );
                assert!(result.is_err());
                result.err()
            },
        );
        report.sample(
            "core_append",
            W,
            n,
            || TimelineProjection::<2>::new(head(n - 1)),
            |p| {
                p.append(head(n - 1), fixture.records[n - 1]).unwrap();
                p.head()
            },
        );
        report.sample(
            "history_append",
            W,
            n,
            || fixture.history(n - 1),
            |h| {
                h.append_input(fixture.records[n - 1], fixture.values[n - 1])
                    .unwrap();
                h.input_count()
            },
        );
        report.sample(
            "ring_append",
            W,
            n,
            || Ring::new(&fixture, n - 1),
            |r| {
                r.append(fixture.values[n - 1]);
                r.next
            },
        );
        let captured = anchor(n, expected);
        report.sample(
            "history_capture",
            W,
            n,
            || fixture.history(n),
            |h| {
                h.capture(captured, &expected).unwrap();
                h.anchor_count()
            },
        );
        report.sample(
            "ring_capture",
            W,
            n,
            || Ring::new(&fixture, n),
            |r| {
                r.snapshots[1] = black_box(expected);
            },
        );
        let history = fixture.history(n);
        report.sample(
            "history_restore",
            W,
            n,
            || State([0; W]),
            |state| {
                history.restore(fixture.anchor, state).unwrap();
                *state
            },
        );
        report.sample(
            "ring_restore",
            W,
            n,
            || (),
            |()| black_box(reference_ring.snapshots[0]),
        );
        for (name, budget) in [("replay", n), ("replay_yield8", 8)] {
            report.sample(
                name,
                W,
                n,
                || fixture.session(n),
                |(_, session)| {
                    while session.phase() == ReplayPhase::Replaying {
                        session
                            .advance(
                                head(n),
                                &Computation::<W>,
                                &fixture,
                                black_box(budget as u64),
                            )
                            .unwrap();
                    }
                    *session.scratch()
                },
            );
            report.sample(
                &format!("ring_{name}"),
                W,
                n,
                || (),
                |()| reference_ring.replay(black_box(n), black_box(budget)),
            );
        }
        report.sample(
            "promote",
            W,
            n,
            || {
                let (live, mut session) = fixture.session(n);
                session
                    .advance(head(n), &Computation::<W>, &fixture, n as u64)
                    .unwrap();
                Some((live, session.validate(head(n), &validator).unwrap()))
            },
            |prepared| {
                let (mut live, ready) = prepared.take().unwrap();
                live.promote(ready).unwrap()
            },
        );
        report.sample(
            "ring_promote",
            W,
            n,
            || Ring::new(&fixture, n),
            |ring| {
                assert!(ring.promote(n as u64, black_box(expected)));
                ring.revision
            },
        );
        report.sample(
            "full_correction",
            W,
            n,
            || fixture.live(n),
            |live| {
                let plan = fixture.plan(n);
                let token = live.reserve(&plan, observation(fixture.anchor)).unwrap();
                let mut session =
                    ReplaySession::<_, (), 0>::begin(plan, token, State([0; W]), &fixture, FX)
                        .unwrap();
                session
                    .advance(head(n), &Computation::<W>, &fixture, n as u64)
                    .unwrap();
                live.promote(session.validate(head(n), &validator).unwrap())
                    .unwrap()
            },
        );
        report.sample(
            "ring_full_correction",
            W,
            n,
            || Ring::new(&fixture, n),
            |ring| {
                let state = ring.replay(n, n);
                assert_eq!(state, expected);
                assert!(ring.promote(n as u64, state));
                ring.revision
            },
        );
        report.sample(
            "competing_corrections",
            W,
            n,
            || fixture.live(n),
            |live| {
                let first = fixture.plan(n);
                let second = fixture.plan(n);
                let a = live.reserve(&first, observation(fixture.anchor)).unwrap();
                let b = live.reserve(&second, observation(fixture.anchor)).unwrap();
                let mut a = ReplaySession::<_, (), 0>::begin(first, a, State([0; W]), &fixture, FX)
                    .unwrap();
                let mut b =
                    ReplaySession::<_, (), 0>::begin(second, b, State([0; W]), &fixture, FX)
                        .unwrap();
                a.advance(head(n), &Computation::<W>, &fixture, n as u64)
                    .unwrap();
                b.advance(head(n), &Computation::<W>, &fixture, n as u64)
                    .unwrap();
                let a = a.validate(head(n), &validator).unwrap();
                let b = b.validate(head(n), &validator).unwrap();
                live.promote(a).unwrap();
                assert_eq!(live.promote(b), Err(TimelineError::Conflict));
                live.read().0
            },
        );
        let mut replacement = fixture.records[0];
        replacement.revision = InputRevision::new(1);
        let replacement_value = 973_u64;
        replacement.digest = replacement_value.digest();
        let inputs = ReplacementInputs {
            fixture: &fixture,
            replacement,
            value: replacement_value,
        };
        let mut corrected = expected;
        let delta = replacement_value
            .wrapping_sub(fixture.values[0])
            .wrapping_mul(3_u64.wrapping_pow((n - 1) as u32));
        for word in &mut corrected.0 {
            *word = word.wrapping_add(delta);
        }
        let corrected_validator = Validator(corrected);
        report.sample(
            "input_correction",
            W,
            n,
            || fixture.live(n),
            |live| {
                let plan = ReplayPlan::prepare(
                    head(n),
                    head(n).revision(),
                    Correction::Input {
                        anchor: fixture.anchor,
                        replacement,
                    },
                    &fixture.records[..n],
                    LIMITS,
                )
                .unwrap();
                let token = live
                    .reserve(
                        &plan,
                        observation(StateAnchor {
                            step: replacement.step,
                            digest: replacement.digest,
                            ..fixture.anchor
                        }),
                    )
                    .unwrap();
                let mut session =
                    ReplaySession::<_, (), 0>::begin(plan, token, State([0; W]), &fixture, FX)
                        .unwrap();
                session
                    .advance(head(n), &Computation::<W>, &inputs, n as u64)
                    .unwrap();
                live.promote(session.validate(head(n), &corrected_validator).unwrap())
                    .unwrap()
            },
        );
    }
}
pub fn run(mut reporter: impl Reporter) {
    reporter.sample("timer_control", 0, 0, || 7_u64, |value| black_box(*value));
    // 8-byte, 1-KiB and 4-KiB complete states; same model and input horizon.
    profiles::<1>(&mut reporter);
    profiles::<128>(&mut reporter);
    profiles::<512>(&mut reporter);
}

fn throughput_profile<const W: usize>() {
    let fixture = Fixture::<W>::new();
    let iterations = 1000;
    for n in [1, 8, 32, 128, 512] {
        let expected = fixture.expected(n);
        let validator = Validator(expected);
        // Every full-operation batch includes fresh publication-owner creation;
        // input metadata/payloads and the admitted snapshot are prebuilt once.
        let start = Instant::now();
        for _ in 0..iterations {
            black_box(black_box(&fixture).plan(black_box(n)));
        }
        println!(
            "throughput,core_prepare,{W},{n},{iterations},{}",
            start.elapsed().as_nanos()
        );
        let full_start = Instant::now();
        for _ in 0..iterations {
            let source = black_box(&fixture);
            let mut live = source.live(n);
            let plan = source.plan(n);
            let token = live.reserve(&plan, observation(source.anchor)).unwrap();
            let mut session =
                ReplaySession::<_, (), 0>::begin(plan, token, State([0; W]), source, FX).unwrap();
            session
                .advance(head(n), &Computation::<W>, source, n as u64)
                .unwrap();
            live.promote(session.validate(head(n), &validator).unwrap())
                .unwrap();
            black_box(live);
        }
        println!(
            "throughput,full_correction,{W},{n},{iterations},{}",
            full_start.elapsed().as_nanos()
        );
    }
}
pub fn throughput() {
    throughput_profile::<1>();
    throughput_profile::<128>();
    throughput_profile::<512>();
}
