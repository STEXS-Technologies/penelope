//! Reproducible pure replay benchmark for a small linear process log.
//!
//! This measures deterministic recovery only. It does not include storage,
//! serialization, broker delivery, or StateChronicle coordination.

#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]

use penelope_domain::{
    ActionId, ContentDigest, DefinitionId, DefinitionVersion, InputId, ProcessId, StepId, TenantId,
};
use penelope_executor::engine::{
    ActionResultObservation, LinearSagaDefinition, LinearSagaEventEnvelope, LinearSagaInput,
    RetryPolicy, StepPlan, replay_ordered,
};
use std::time::Instant;

fn id<T: TryFrom<&'static str>>(value: &'static str) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(value).expect("benchmark identifier is valid")
}

fn main() {
    let definition = LinearSagaDefinition::new(
        id::<DefinitionId>("def_benchmark_replay"),
        id::<DefinitionVersion>("dfv_one"),
        ContentDigest([7; 32]),
        vec![StepPlan::canonical_command(
            id::<StepId>("stp_benchmark"),
            ContentDigest([8; 32]),
            RetryPolicy::no_retry(),
        )],
    );
    let start = LinearSagaInput::Start {
        input_id: id::<InputId>("inp_replay_start"),
        action_id: id::<ActionId>("act_replay_start"),
    }
    .to_event(&definition);
    let result = LinearSagaInput::ActionResult {
        input_id: id::<InputId>("inp_replay_result"),
        observation: ActionResultObservation::succeeded(id("act_replay_start")),
        next_action_id: None,
    }
    .to_event(&definition);
    let log = vec![
        LinearSagaEventEnvelope {
            sequence: 0,
            event: start,
        },
        LinearSagaEventEnvelope {
            sequence: 1,
            event: result,
        },
    ];

    let iterations = 1_000_000_u64;
    let tenant_id = id::<TenantId>("tnt_benchmark_replay");
    let process_id = id::<ProcessId>("prc_benchmark_replay");
    let began = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(
            replay_ordered(&definition, &tenant_id, &process_id, &log)
                .expect("benchmark log replays"),
        );
    }
    let elapsed = began.elapsed();
    let nanos_per_replay = elapsed.as_nanos() / u128::from(iterations);
    println!("linear-saga replay: {iterations} logs in {elapsed:?} ({nanos_per_replay} ns/replay)");
}
