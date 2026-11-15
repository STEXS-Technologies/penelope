//! Hosted measurement only: no runtime or benchmark dependency in the portable library.
//! Latencies include context construction, publication and draining (no effect execution).
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
#[path = "../tests/support/allocations.rs"]
mod allocations;
use allocations::{Counts, start_counting, stop_counting};
use penelope_core::machine::*;
use std::{hint::black_box, time::Instant};

const SAMPLES: usize = 101;
const BATCH: usize = 32;
const CAPACITY: usize = 32;

const fn definition() -> MachineDefinition {
    MachineDefinition {
        fingerprint: MachineDefinitionId::from_bytes([3; 32]),
        state_schema: 1,
        event_schema: 1,
    }
}
const fn scope() -> MachineScope {
    MachineScope {
        tenant: MachineTenantId::from_bytes([1; 32]),
        instance: MachineId::from_bytes([2; 32]),
        definition: definition(),
    }
}
#[derive(Clone, Copy)]
struct Profile {
    states: usize,
    events: usize,
    intents: usize,
    reject: bool,
    quota: bool,
}
impl<const WORDS: usize> Machine for SizedModel<WORDS> {
    type State = [u64; WORDS];
    type Event = usize;
    type Effect = u64;
    type Error = MachineError;
    fn definition(&self) -> MachineDefinition {
        definition()
    }
    fn validate(&self, state: &Self::State) -> Result<(), MachineError> {
        if state[0] < self.0.states as u64 {
            Ok(())
        } else {
            Err(MachineError::Capacity)
        }
    }
    fn transition<S: IntentSink<u64>>(
        &self,
        current: &Self::State,
        event: &usize,
        context: &TransitionContext,
        sink: &mut S,
    ) -> Result<Transition<Self::State>, MachineError> {
        let profile = self.0;
        // Consumer-defined linear dispatch; the kernel does not own a state/event table.
        let state = (0..profile.states)
            .find(|state| *state as u64 == current[0])
            .ok_or(MachineError::Capacity)?;
        let selected = (0..profile.events)
            .find(|candidate| candidate == event)
            .ok_or(MachineError::Capacity)?;
        if profile.reject {
            return Ok(Transition::Rejected {
                reason: RejectCode::new(1),
            });
        }
        for index in 0..profile.intents {
            let mut identity = context.event.bytes();
            identity[31] = u8::try_from(index).map_err(|_conversion| MachineError::Capacity)?;
            sink.stage(ProposedIntent {
                metadata: IntentMetadata {
                    identity: MachineIntentId::from_bytes(identity),
                    digest: context.event_digest,
                    payload_bytes: 8,
                },
                payload: selected as u64,
            })?;
        }
        let mut next = *black_box(current);
        next[0] = ((state + selected + 1) % profile.states) as u64;
        Ok(Transition::Accepted {
            next,
            transition: TransitionId::new(1),
        })
    }
}
struct SizedModel<const WORDS: usize>(Profile);

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
fn operation<const WORDS: usize>(
    owner: &mut MemoryMachine<SizedModel<WORDS>, CAPACITY>,
    profile: Profile,
    step: u64,
) {
    let event = black_box(step as usize % profile.events);
    let before = owner.read().0;
    let outcome = owner.handle(&event, context(before, step));
    black_box(outcome).unwrap_or_else(|error| {
        assert!(profile.quota && matches!(error, MachineFailure::Kernel(MachineError::Capacity)));
        HandleOutcome::Unhandled
    });
    black_box(owner.read().1);
    black_box(owner.take_intents());
}
fn profile<const WORDS: usize>(profile: Profile) {
    let limit = if profile.quota {
        profile.intents - 1
    } else {
        CAPACITY
    };
    let mut owner = MemoryMachine::new(
        SizedModel::<WORDS>(profile),
        scope(),
        [0; WORDS],
        IntentLimits {
            max_intents: limit,
            max_payload_bytes: 256,
        },
    )
    .unwrap();
    // Independent one-step oracle before timing: outcome, state, head, payload and rollback.
    let before = owner.read().0;
    let result = owner.handle(&1, context(before, 1));
    if profile.quota {
        assert!(matches!(
            result,
            Err(MachineFailure::Kernel(MachineError::Capacity))
        ));
    } else if profile.reject {
        assert_eq!(
            result.unwrap(),
            HandleOutcome::Rejected {
                reason: RejectCode::new(1)
            }
        );
    } else {
        assert!(matches!(result.unwrap(), HandleOutcome::Accepted(_)));
    }
    let changed = !profile.quota && !profile.reject;
    assert_eq!(
        owner.read().1[0],
        if changed {
            (2 % profile.states) as u64
        } else {
            0
        }
    );
    assert_eq!(owner.read().0 == before, !changed);
    assert!(owner.read().1[1..].iter().all(|word| *word == 0));
    let intents = owner.take_intents();
    assert_eq!(intents.len(), if changed { profile.intents } else { 0 });
    assert!(intents.iter().all(|intent| intent.intent.payload == 1));
    for step in 0..128 {
        operation(&mut owner, profile, step);
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let start = Instant::now();
        for offset in 0..BATCH {
            operation(&mut owner, profile, (sample * BATCH + offset) as u64);
        }
        samples.push(start.elapsed().as_nanos());
    }
    // Allocation instrumentation is disabled throughout latency sampling.
    start_counting();
    for step in 0..BATCH {
        operation(&mut owner, profile, step as u64);
    }
    let counts = stop_counting();
    assert_eq!(
        counts,
        Counts {
            allocations: 0,
            reallocations: 0,
            deallocations: 0,
            requested_bytes: 0
        }
    );
    let operations = 1 + 128 + SAMPLES * BATCH + BATCH;
    assert_eq!(
        owner.read().0.revision,
        MachineRevision::new(if changed { operations as u64 } else { 0 })
    );
    let mut expected = [0; WORDS];
    if changed {
        let increments = [128, SAMPLES * BATCH, BATCH]
            .into_iter()
            .flat_map(|count| 0..count)
            .map(|step| step % profile.events + 1)
            .sum::<usize>();
        expected[0] = ((2 + increments) % profile.states) as u64;
    }
    assert_eq!(owner.read().1, &expected);
    let raw = samples
        .iter()
        .map(u128::to_string)
        .collect::<Vec<_>>()
        .join(";");
    samples.sort_unstable();
    let total: u128 = samples.iter().sum();
    let throughput = (SAMPLES * BATCH) as u128 * 1_000_000_000 / total;
    println!(
        "flat,{WORDS},{},{},{},{},{},{},{},{},{},{total},{SAMPLES},{BATCH},{throughput},{raw}",
        profile.states,
        profile.events,
        profile.intents,
        profile.reject,
        profile.quota,
        samples[50] / BATCH as u128,
        samples[95] / BATCH as u128,
        samples[99] / BATCH as u128,
        counts.allocations
    );
}
fn matrix<const WORDS: usize>() {
    for states in [2, 8, 32] {
        for events in [2, 8, 32] {
            for intents in [0, 1, 8, 32] {
                for reject in [false, true] {
                    profile::<WORDS>(Profile {
                        states,
                        events,
                        intents,
                        reject,
                        quota: false,
                    });
                }
                if intents != 0 {
                    profile::<WORDS>(Profile {
                        states,
                        events,
                        intents,
                        reject: false,
                        quota: true,
                    });
                }
            }
        }
    }
}
fn main() {
    start_counting();
    let control = black_box(Vec::<u8>::with_capacity(16));
    drop(black_box(control));
    assert_eq!(
        stop_counting(),
        Counts {
            allocations: 1,
            reallocations: 0,
            deallocations: 1,
            requested_bytes: 16
        }
    );
    println!(
        "kind,state_words,states,events,intents,reject,quota,batch_mean_p50_ns,batch_mean_p95_ns,batch_mean_p99_ns,allocations_per_op,total_elapsed_ns,samples,batch,operations_per_second,raw_batch_elapsed_ns"
    );
    matrix::<1>();
    matrix::<32>();
    matrix::<512>();
    matrix::<8192>();
}
