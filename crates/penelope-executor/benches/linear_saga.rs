//! Reproducible pure linear-saga transition benchmark.

use std::{hint::black_box, time::Instant};

use penelope_domain::{
    ActionId, ContentDigest, DomainError, ProcessActionKindV1, ProcessId, StepId, TenantId,
};
use penelope_executor::engine::{
    ActionResultObservationV1, EngineError, LinearSagaDefinitionV1, RetryPolicyV1, StepPlanV1,
    apply_action_result, start,
};
use thiserror::Error;

#[derive(Debug, Error)]
enum BenchmarkError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("new saga did not plan an initial action")]
    MissingInitialAction,
    #[error("benchmark iteration count cannot calculate a per-operation value")]
    InvalidIterationCount,
}

fn identifier<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn main() -> Result<(), BenchmarkError> {
    let iterations = 100_000_u32;
    let definition = LinearSagaDefinitionV1 {
        steps: vec![StepPlanV1 {
            step_id: identifier::<StepId>("stp_benchmark")?,
            action_kind: ProcessActionKindV1::CanonicalCommand,
            payload_digest: ContentDigest([1; 32]),
            retry_policy: RetryPolicyV1::no_retry(),
            compensation: None,
        }],
    };
    let tenant_id = identifier::<TenantId>("tnt_benchmark")?;
    let process_id = identifier::<ProcessId>("prc_benchmark")?;
    let action_id = identifier::<ActionId>("act_benchmark")?;
    let started_at = Instant::now();

    for _ in 0..iterations {
        let started = black_box(start(
            black_box(&definition),
            tenant_id.clone(),
            process_id.clone(),
            action_id.clone(),
        )?);
        let action = started
            .next_action
            .as_ref()
            .ok_or(BenchmarkError::MissingInitialAction)?;
        let completed = apply_action_result(
            &definition,
            &started.projection,
            tenant_id.clone(),
            process_id.clone(),
            &ActionResultObservationV1::succeeded(action.action_id.clone()),
            None,
        )?;
        black_box(completed);
    }

    let elapsed = started_at.elapsed();
    let nanos_per_operation = elapsed
        .as_nanos()
        .checked_div(u128::from(iterations))
        .ok_or(BenchmarkError::InvalidIterationCount)?;
    println!(
        "linear-saga start/result/complete: {iterations} operations in {elapsed:?} ({nanos_per_operation} ns/op)"
    );
    Ok(())
}
