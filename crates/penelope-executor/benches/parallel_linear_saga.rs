//! Reproducible parallel pure linear-saga throughput benchmark.
//!
//! Each worker executes isolated in-memory transitions. This is intentionally
//! not a claim about shared-state coordination, adapters, storage, network
//! effects, or end-to-end throughput. Benchmark failures use a `thiserror`
//! enum like the library's production error boundaries.

use std::{hint::black_box, thread, time::Instant};

use penelope_domain::{
    ActionId, ContentDigest, DefinitionId, DefinitionVersion, DomainError, ProcessActionKindV1,
    ProcessId, StepId, TenantId,
};
use penelope_executor::engine::{
    ActionResultObservationV1, EngineError, LinearSagaDefinitionV1, RetryPolicyV1, StepPlanV1,
    apply_action_result, start,
};
use thiserror::Error;

const ITERATIONS_PER_WORKER: u32 = 100_000;

#[derive(Debug, Error)]
enum BenchmarkError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("new saga did not plan an initial action")]
    MissingInitialAction,
    #[error("available parallelism cannot fit the benchmark worker bound")]
    WorkerCountOverflow,
    #[error("parallel benchmark worker panicked")]
    WorkerPanicked,
    #[error("parallel benchmark operation count overflowed")]
    OperationCountOverflow,
    #[error("benchmark iteration count cannot calculate a per-operation value")]
    InvalidIterationCount,
}

fn identifier<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn run_worker(
    definition: &LinearSagaDefinitionV1,
    tenant_id: &TenantId,
    process_id: &ProcessId,
    action_id: &ActionId,
) -> Result<(), BenchmarkError> {
    for _ in 0..ITERATIONS_PER_WORKER {
        let started = black_box(start(
            black_box(definition),
            tenant_id.clone(),
            process_id.clone(),
            action_id.clone(),
        )?);
        let action = started
            .next_action
            .as_ref()
            .ok_or(BenchmarkError::MissingInitialAction)?;
        black_box(apply_action_result(
            definition,
            &started.projection,
            tenant_id.clone(),
            process_id.clone(),
            &ActionResultObservationV1::succeeded(action.action_id.clone()),
            None,
        )?);
    }
    Ok(())
}

fn main() -> Result<(), BenchmarkError> {
    let workers = u32::try_from(thread::available_parallelism().map_or(1, std::num::NonZero::get))
        .map_err(|_error| BenchmarkError::WorkerCountOverflow)?;
    let total_operations = workers
        .checked_mul(ITERATIONS_PER_WORKER)
        .ok_or(BenchmarkError::OperationCountOverflow)?;
    let definition = LinearSagaDefinitionV1 {
        definition_id: identifier::<DefinitionId>("def_parallel_benchmark")?,
        definition_version: identifier::<DefinitionVersion>("dfv_one")?,
        definition_digest: ContentDigest([99; 32]),
        steps: vec![StepPlanV1 {
            step_id: identifier::<StepId>("stp_parallel_benchmark")?,
            action_kind: ProcessActionKindV1::CanonicalCommand,
            payload_digest: ContentDigest([1; 32]),
            retry_policy: RetryPolicyV1::no_retry(),
            compensation: None,
        }],
    };
    let tenant_id = identifier::<TenantId>("tnt_parallel_benchmark")?;
    let process_id = identifier::<ProcessId>("prc_parallel_benchmark")?;
    let action_id = identifier::<ActionId>("act_parallel_benchmark")?;
    let started_at = Instant::now();

    thread::scope(|scope| -> Result<(), BenchmarkError> {
        let handles = (0..workers)
            .map(|_| scope.spawn(|| run_worker(&definition, &tenant_id, &process_id, &action_id)))
            .collect::<Vec<_>>();
        for handle in handles {
            handle
                .join()
                .map_err(|_panic_payload| BenchmarkError::WorkerPanicked)??;
        }
        Ok(())
    })?;

    let elapsed = started_at.elapsed();
    let nanos_per_operation = elapsed
        .as_nanos()
        .checked_div(u128::from(total_operations))
        .ok_or(BenchmarkError::InvalidIterationCount)?;
    let operations_per_second = u128::from(total_operations)
        .checked_mul(1_000_000_000)
        .and_then(|operations| operations.checked_div(elapsed.as_nanos()))
        .ok_or(BenchmarkError::InvalidIterationCount)?;
    println!(
        "parallel linear-saga start/result/complete: {total_operations} isolated operations across {workers} workers in {elapsed:?} ({nanos_per_operation} ns/op, {operations_per_second} ops/s)"
    );
    Ok(())
}
