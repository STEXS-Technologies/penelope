//! Same computation through the native timeline and its machine adapter.
//! Individual call samples; fixture construction and result checks are untimed.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::missing_const_for_fn
)]
#[path = "support/allocator.rs"]
mod allocator;
use penelope_core::{machine::*, task::Digest};
use penelope_timeline::*;
use std::{fmt::Debug, hint::black_box, time::Instant};

const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 1,
};
const MACHINE_SCOPE: MachineScope = MachineScope {
    tenant: MachineTenantId::from_bytes([1; 32]),
    instance: MachineId::from_bytes([2; 32]),
    definition: DEFINITION,
};
const SCOPE: TimelineScope = TimelineScope {
    tenant: TimelineTenantId::from_digest(Digest([11; 32])),
    timeline: TimelineId::from_digest(Digest([12; 32])),
    model: TimelineModelId::from_digest(Digest([13; 32])),
    schema: TimelineSchemaId::from_digest(Digest([14; 32])),
};
const SAMPLES: usize = if cfg!(debug_assertions) { 3 } else { 101 };
const WARMUP: usize = if cfg!(debug_assertions) { 1 } else { 8 };
#[derive(Clone)]
struct State<const W: usize>([u64; W]);
impl<const W: usize> HistoryPayload for State<W> {
    fn digest(&self) -> Digest {
        let mut bytes = [[0_u8; 8]; W];
        // Hash all words without heap serialization. Hash the complete byte
        // array below, rather than sampling a prefix of the application state.
        for (encoded, word) in bytes.iter_mut().zip(self.0) {
            *encoded = word.to_le_bytes();
        }
        Digest::of(bytes.as_flattened())
    }
    fn byte_len(&self) -> u64 {
        (W * 8) as u64
    }
}
fn compute<const W: usize>(state: &mut State<W>, amount: u64) {
    for (index, word) in state.0.iter_mut().enumerate() {
        *word += amount + index as u64;
    }
}
fn verify<const W: usize>(state: &State<W>, base: u64, steps: u64) {
    let sum = steps * (steps + 1) / 2;
    for (index, word) in state.0.iter().enumerate() {
        assert_eq!(*word, base + sum + index as u64 * steps);
    }
    black_box(state);
}
struct Counter<const W: usize>;
impl<const W: usize> Machine for Counter<W> {
    type State = State<W>;
    type Event = u64;
    type Effect = ();
    type Error = MachineError;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, _: &State<W>) -> Result<(), MachineError> {
        Ok(())
    }
    fn transition<S: IntentSink<()>>(
        &self,
        current: &State<W>,
        event: &u64,
        _: &TransitionContext,
        _: &mut S,
    ) -> Result<Transition<State<W>>, MachineError> {
        let mut next = current.clone();
        compute(&mut next, *event);
        Ok(Transition::Accepted {
            next,
            transition: TransitionId::new(1),
        })
    }
}
impl<const W: usize> RewindableComputation for Counter<W> {
    type State = State<W>;
    type Input = u64;
    type Effect = ();
    type Error = TimelineError;
    fn step<S: EffectSink<Effect = ()>>(
        &self,
        state: &mut State<W>,
        input: &u64,
        _: StepContext,
        _: &mut S,
    ) -> Result<(), TimelineError> {
        compute(state, *input);
        Ok(())
    }
}
fn machine_head(revision: u64) -> MachineHead {
    MachineHead {
        scope: MACHINE_SCOPE,
        revision: MachineRevision::new(revision),
    }
}
fn observe<T>(probe: bool, operation: impl FnOnce() -> T) -> (T, u128) {
    if probe {
        let (result, allocations, reallocations, bytes) = allocator::measure(operation);
        assert_eq!((allocations, reallocations, bytes), (0, 0, 0));
        (result, 0)
    } else {
        let start = Instant::now();
        let result = black_box(operation());
        (result, start.elapsed().as_nanos())
    }
}
fn report(path: &str, width: usize, horizon: usize, operation: &str, mut samples: Vec<u128>) {
    assert_eq!(samples.len(), SAMPLES);
    let raw = samples
        .iter()
        .map(u128::to_string)
        .collect::<Vec<_>>()
        .join(";");
    samples.sort_unstable();
    println!(
        "{path},{width},{horizon},{operation},{},{},{},{SAMPLES},0,0,0,{raw}",
        samples[(SAMPLES * 50).div_ceil(100) - 1],
        samples[(SAMPLES * 95).div_ceil(100) - 1],
        samples[(SAMPLES * 99).div_ceil(100) - 1]
    );
}
fn profile<M>(
    path: &str,
    width: usize,
    horizon: usize,
    model: &M,
    make_state: impl Fn(u64) -> M::State,
    make_input: impl Fn(u64) -> M::Input,
    check: impl Fn(&M::State, u64, u64),
) where
    M: RewindableComputation,
    M::State: Clone + HistoryPayload,
    M::Input: HistoryPayload,
    M::Error: Debug,
{
    let mut live_samples = Vec::with_capacity(SAMPLES);
    let mut correction_samples = Vec::with_capacity(SAMPLES);
    let valid = |_: &M::State, _: TimelineHead| Ok(());
    for sample in 0..WARMUP + SAMPLES + 1 {
        let probe = sample == WARMUP + SAMPLES;
        let initial = black_box(make_state(0));
        let genesis = TimelineHead::new(
            SCOPE,
            TimelineBranchId::ZERO,
            TimelineStep::ZERO,
            TimelineRevision::ZERO,
            None,
        )
        .unwrap();
        let mut live = LiveTimeline::<_, M::Effect, 1, 0>::new(
            genesis,
            initial.clone(),
            EffectLimits {
                max_intents: 0,
                max_payload_bytes: 0,
            },
        );
        let mut retained = Vec::with_capacity(horizon);
        let mut live_ns = 0;
        for step in 1..=horizon {
            let input = black_box(make_input(step as u64));
            let mut id = [0; 32];
            id[..8].copy_from_slice(&(step as u64).to_le_bytes());
            let mut apply = || {
                live.apply_input(
                    model,
                    TimelineInputId::from_digest(Digest(id)),
                    &input,
                    &valid,
                )
                .unwrap()
            };
            let record = if step == horizon {
                let (record, nanos) = observe(probe, apply);
                live_ns = nanos;
                record
            } else {
                apply()
            };
            retained.push(CapturedInput::new(record, input).unwrap());
        }
        check(live.read().1, 0, horizon as u64);
        let authority_state = black_box(make_state(100));
        let snapshot = CapturedSnapshot::capture(
            live.read().0,
            TimelineStep::ZERO,
            TimelineSnapshotId::from_digest(authority_state.digest()),
            authority_state,
        )
        .unwrap();
        let records: Vec<_> = retained.iter().map(CapturedInput::record).collect();
        let ((), correction_ns) = observe(probe, || {
            let mut correction = live
                .begin_correction(
                    CorrectionRequest {
                        correction: Correction::State(snapshot.anchor()),
                        inputs: &records,
                        observation: AuthorityObservation {
                            scope: SCOPE,
                            authority: TimelineAuthorityId::from_digest(Digest([99; 32])),
                            epoch: AuthorityEpoch::ZERO,
                            sequence: AuthoritySequence::new(1),
                            step: TimelineStep::ZERO,
                            digest: snapshot.anchor().digest,
                        },
                        scratch: initial,
                        limits: ReplayLimits {
                            max_retained_inputs: horizon as u64,
                            max_replay_steps: horizon as u64,
                        },
                    },
                    &snapshot,
                )
                .unwrap();
            while correction.advance(model, retained.as_slice(), 8).unwrap()
                == ReplayPhase::Replaying
            {}
            correction.publish(&valid).unwrap();
        });
        let (head, state) = live.read();
        check(state, 100, horizon as u64);
        assert_eq!(head.step().get(), horizon as u64);
        assert_eq!(head.revision().get(), horizon as u64 + 1);
        assert_eq!(live.eligible_effects().count(), 0);
        if (WARMUP..WARMUP + SAMPLES).contains(&sample) {
            live_samples.push(live_ns);
            correction_samples.push(correction_ns);
        }
    }
    report(path, width, horizon, "live_input", live_samples);
    report(path, width, horizon, "correction", correction_samples);
}
fn run<const W: usize>() {
    let adapter = MachineTimeline::new(Counter::<W>, MACHINE_SCOPE, SCOPE).unwrap();
    for horizon in [1, 8, 32, 128] {
        profile(
            "native",
            W * 8,
            horizon,
            &Counter::<W>,
            |base| State([base; W]),
            |step| step,
            verify,
        );
        profile(
            "machine",
            W * 8,
            horizon,
            &adapter,
            |base| adapter.restore(machine_head(0), State([base; W])).unwrap(),
            |step| MachineTimelineInput {
                context: TransitionContext {
                    expected: machine_head(step - 1),
                    event: MachineEventId::from_bytes(Digest::of(&step.to_le_bytes()).0),
                    event_digest: step.digest().0,
                    logical_time: MachineLogicalTime::new(step),
                },
                event: step,
            },
            |state, base, steps| {
                let (head, values) = state.read();
                verify(values, base, steps);
                assert_eq!(head, machine_head(steps));
                let last = state.last_transition().unwrap();
                assert_eq!(last.context.expected, machine_head(steps - 1));
                assert_eq!(last.revision.get(), steps);
                assert_eq!(last.transition, TransitionId::new(1));
            },
        );
    }
}
fn main() {
    let (positive, allocations, reallocations, bytes) =
        allocator::measure(|| black_box(Vec::<u8>::with_capacity(16)));
    assert_eq!((allocations, reallocations, bytes), (1, 0, 16));
    drop(positive);
    println!(
        "path,state_bytes,horizon,operation,p50_ns,p95_ns,p99_ns,samples,allocations,reallocations,requested_bytes,raw_ns"
    );
    run::<1>();
    run::<32>();
    run::<512>();
    run::<8192>();
}
