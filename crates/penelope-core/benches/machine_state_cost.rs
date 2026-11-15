//! State ownership cost, separate from dispatch depth, intents and durable I/O.
//! The direct consumer path supplies no publication or validation guarantees.
#![allow(
    clippy::unwrap_used,
    clippy::unreachable,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
#[path = "support/measurement.rs"]
mod measurement;
use core::convert::Infallible;
use measurement::{BATCH, SAMPLES};
use penelope_core::machine::*;
use std::hint::black_box;

const DEFINITION: MachineDefinition = MachineDefinition {
    fingerprint: MachineDefinitionId::from_bytes([3; 32]),
    state_schema: 1,
    event_schema: 1,
};
const SCOPE: MachineScope = MachineScope {
    tenant: MachineTenantId::from_bytes([1; 32]),
    instance: MachineId::from_bytes([2; 32]),
    definition: DEFINITION,
};
struct Counter<const WORDS: usize>;
impl<const WORDS: usize> Machine for Counter<WORDS> {
    type State = [u64; WORDS];
    type Event = u64;
    type Effect = Infallible;
    type Error = Infallible;
    fn definition(&self) -> MachineDefinition {
        DEFINITION
    }
    fn validate(&self, _: &Self::State) -> Result<(), Infallible> {
        Ok(())
    }
    fn transition<S: IntentSink<Infallible>>(
        &self,
        current: &Self::State,
        event: &u64,
        _: &TransitionContext,
        _: &mut S,
    ) -> Result<Transition<Self::State>, Infallible> {
        let mut next = *black_box(current);
        next[0] = next[0].wrapping_add(*event);
        Ok(Transition::Accepted {
            next,
            transition: TransitionId::new(1),
        })
    }
}
struct NoIntents;
impl IntentSink<Infallible> for NoIntents {
    fn stage(&mut self, intent: ProposedIntent<Infallible>) -> Result<(), MachineError> {
        match intent.payload {}
    }
    fn status(&self) -> Result<(), MachineError> {
        Ok(())
    }
}
fn context(head: MachineHead, step: u64) -> TransitionContext {
    let mut bytes = [0; 32];
    bytes[..8].copy_from_slice(&step.to_be_bytes());
    TransitionContext {
        expected: head,
        event: MachineEventId::from_bytes(bytes),
        event_digest: bytes,
        logical_time: MachineLogicalTime::new(step),
    }
}
fn initial<const WORDS: usize>() -> [u64; WORDS] {
    core::array::from_fn(|index| index as u64)
}
fn expected<const WORDS: usize>() -> [u64; WORDS] {
    let mut expected = initial::<WORDS>();
    // Closed-form oracle independent of the counter implementation. The sampler
    // visits 0..count separately for warmup, timing and allocation inspection.
    expected[0] = [128, SAMPLES * BATCH, BATCH]
        .into_iter()
        .map(|count| (count * (count + 1) / 2) as u64)
        .sum();
    expected
}
fn profile<const WORDS: usize>() {
    let mut copied = initial::<WORDS>();
    let copy = measurement::measure(|step| {
        let mut next = black_box(*black_box(&copied));
        next[0] = next[0].wrapping_add(black_box(step + 1));
        copied = next;
        black_box(&copied);
    });
    assert_eq!(copied, expected::<WORDS>());
    copy.report(&format!("owned_copy_and_counter,{WORDS}"));

    let mut state = initial::<WORDS>();
    let consumer = measurement::measure(|step| {
        let supplied = context(
            MachineHead {
                scope: SCOPE,
                revision: MachineRevision::new(step),
            },
            step,
        );
        let Transition::Accepted { next, .. } = Counter::<WORDS>
            .transition(
                black_box(&state),
                &black_box(step + 1),
                &supplied,
                &mut NoIntents,
            )
            .unwrap()
        else {
            unreachable!("counter always accepts")
        };
        state = next;
        black_box(&state);
    });
    assert_eq!(state, expected::<WORDS>());
    consumer.report(&format!("consumer_transition,{WORDS}"));

    let mut owner = MemoryMachine::<_, 0>::new(
        Counter::<WORDS>,
        SCOPE,
        initial::<WORDS>(),
        IntentLimits {
            max_intents: 0,
            max_payload_bytes: 0,
        },
    )
    .unwrap();
    let machine = measurement::measure(|step| {
        let supplied = context(owner.read().0, step);
        let outcome = owner.handle(&black_box(step + 1), supplied).unwrap();
        black_box(outcome);
        black_box(owner.read().1);
    });
    assert_eq!(owner.read().1, &expected::<WORDS>());
    assert_eq!(
        owner.read().0.revision,
        MachineRevision::new((128 + SAMPLES * BATCH + BATCH) as u64)
    );
    assert_eq!(owner.pending_intents().count(), 0);
    machine.report(&format!("machine_publication,{WORDS}"));
}
fn main() {
    measurement::positive_control();
    println!(
        "kind,state_words,batch_mean_p50_ns,batch_mean_p95_ns,batch_mean_p99_ns,allocations_per_op,total_elapsed_ns,samples,batch,operations_per_second,raw_batch_elapsed_ns"
    );
    profile::<1>();
    profile::<32>();
    profile::<512>();
    profile::<8192>();
}
