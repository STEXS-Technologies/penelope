//! Parallel pure bounded-graph throughput benchmark for isolated processes.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::map_err_ignore
)]

use std::{hint::black_box, thread, time::Instant};

use penelope_domain::{ContentDigest, DomainError, ProcessId, TenantId};
use penelope_executor::engine::{
    ActionResultObservationV1, GraphTransitionOutcomeV1, GraphTransitionV1,
    ProcessGraphDefinitionV1, RetryPolicyV1, StepPlanV1,
};
use penelope_executor::graph::{GraphSagaInputV1, apply_graph_result, start_graph};
use thiserror::Error;

const ITERATIONS_PER_WORKER: u32 = 100_000;

#[derive(Debug, Error)]
enum BenchmarkError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Graph(#[from] penelope_executor::graph::GraphEngineError),
    #[error("benchmark action was not planned")]
    MissingAction,
    #[error("benchmark worker panicked")]
    WorkerPanicked,
}

fn id<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn worker(
    definition: &ProcessGraphDefinitionV1,
    tenant: &TenantId,
    process: &ProcessId,
) -> Result<(), BenchmarkError> {
    for _ in 0..ITERATIONS_PER_WORKER {
        let started = start_graph(
            definition,
            tenant.clone(),
            process.clone(),
            GraphSagaInputV1::Start {
                input_id: id("inp_parallel_start")?,
                action_id: id("act_parallel_first")?,
            },
        )?;
        let first = started
            .next_action
            .as_ref()
            .ok_or(BenchmarkError::MissingAction)?;
        let advanced = apply_graph_result(
            definition,
            &started.projection,
            GraphSagaInputV1::ActionResult {
                input_id: id("inp_parallel_one")?,
                observation: ActionResultObservationV1::succeeded(first.action_id.clone()),
                next_action_id: Some(id("act_parallel_second")?),
            },
        )?;
        let second = advanced
            .next_action
            .as_ref()
            .ok_or(BenchmarkError::MissingAction)?;
        black_box(apply_graph_result(
            definition,
            &advanced.projection,
            GraphSagaInputV1::ActionResult {
                input_id: id("inp_parallel_two")?,
                observation: ActionResultObservationV1::succeeded(second.action_id.clone()),
                next_action_id: None,
            },
        )?);
    }
    Ok(())
}

fn main() -> Result<(), BenchmarkError> {
    let definition = ProcessGraphDefinitionV1 {
        definition_id: id("def_parallel_graph")?,
        definition_version: id("dfv_one")?,
        definition_digest: ContentDigest([27; 32]),
        steps: vec![
            StepPlanV1::canonical_command(
                id("stp_first")?,
                ContentDigest([28; 32]),
                RetryPolicyV1::no_retry(),
            ),
            StepPlanV1::canonical_command(
                id("stp_second")?,
                ContentDigest([29; 32]),
                RetryPolicyV1::no_retry(),
            ),
        ],
        entry_step_id: id("stp_first")?,
        transitions: vec![
            GraphTransitionV1 {
                from_step: id("stp_first")?,
                on: GraphTransitionOutcomeV1::Succeeded,
                to_step: Some(id("stp_second")?),
            },
            GraphTransitionV1 {
                from_step: id("stp_second")?,
                on: GraphTransitionOutcomeV1::Succeeded,
                to_step: None,
            },
        ],
        max_step_visits: std::num::NonZeroU32::new(2).ok_or(BenchmarkError::MissingAction)?,
    };
    let workers = thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let tenant = id::<TenantId>("tnt_parallel_graph")?;
    let process = id::<ProcessId>("prc_parallel_graph")?;
    let total = u128::from(ITERATIONS_PER_WORKER) * workers as u128;
    let began = Instant::now();
    thread::scope(|scope| -> Result<(), BenchmarkError> {
        let handles = (0..workers)
            .map(|_| scope.spawn(|| worker(&definition, &tenant, &process)))
            .collect::<Vec<_>>();
        for handle in handles {
            handle
                .join()
                .map_err(|_| BenchmarkError::WorkerPanicked)??;
        }
        Ok(())
    })?;
    let elapsed = began.elapsed();
    println!(
        "parallel graph-saga: {total} isolated operations across {workers} workers in {elapsed:?} ({:.0} ops/s)",
        total as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}
