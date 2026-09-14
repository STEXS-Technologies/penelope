//! Reproducible pure replay benchmark for a small linear process log.
//!
//! This measures deterministic recovery only. It does not include storage,
//! serialization, broker delivery, or StateChronicle coordination.

use penelope_domain::{
    ActionId, ContentDigest, DefinitionId, DefinitionVersion, InputId, ProcessId, StepId, TenantId,
};
use penelope_executor::engine::{
    ActionResultObservationV1, LinearSagaDefinitionV1, LinearSagaEventEnvelopeV1,
    LinearSagaInputV1, RetryPolicyV1, StepPlanV1, replay_ordered,
};
use std::time::Instant;

fn id<T: TryFrom<&'static str>>(value: &'static str) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(value).expect("benchmark identifier is valid")
}

fn main() {
    let definition = LinearSagaDefinitionV1::new(
        id::<DefinitionId>("def_benchmark_replay"),
        id::<DefinitionVersion>("dfv_one"),
        ContentDigest([7; 32]),
        vec![StepPlanV1::canonical_command(
            id::<StepId>("stp_benchmark"),
            ContentDigest([8; 32]),
            RetryPolicyV1::no_retry(),
        )],
    );
    let start = LinearSagaInputV1::Start {
        input_id: id::<InputId>("inp_replay_start"),
        action_id: id::<ActionId>("act_replay_start"),
    }
    .to_event(&definition);
    let result = LinearSagaInputV1::ActionResult {
        input_id: id::<InputId>("inp_replay_result"),
        observation: ActionResultObservationV1::succeeded(id("act_replay_start")),
        next_action_id: None,
    }
    .to_event(&definition);
    let log = vec![
        LinearSagaEventEnvelopeV1 {
            sequence: 0,
            event: start,
        },
        LinearSagaEventEnvelopeV1 {
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
