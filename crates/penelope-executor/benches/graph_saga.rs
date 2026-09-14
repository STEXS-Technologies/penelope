//! Reproducible pure bounded-graph transition benchmark.

use std::{hint::black_box, time::Instant};

use penelope_domain::{ActionId, ContentDigest, DomainError, InputId, ProcessId, TenantId};
use penelope_executor::engine::{
    GraphTransitionOutcomeV1, GraphTransitionV1, ProcessGraphDefinitionV1, RetryPolicyV1,
    StepPlanV1,
};
use penelope_executor::graph::{GraphSagaInputV1, apply_graph_result, start_graph};
use thiserror::Error;

#[derive(Debug, Error)]
enum BenchmarkError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Engine(#[from] penelope_executor::engine::GraphDefinitionError),
    #[error(transparent)]
    Graph(#[from] penelope_executor::graph::GraphEngineError),
    #[error("benchmark graph did not plan an action")]
    MissingAction,
}

fn id<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn main() -> Result<(), BenchmarkError> {
    let definition = ProcessGraphDefinitionV1 {
        definition_id: id("def_graph_benchmark")?,
        definition_version: id("dfv_one")?,
        definition_digest: ContentDigest([17; 32]),
        steps: vec![
            StepPlanV1::canonical_command(
                id("stp_first")?,
                ContentDigest([18; 32]),
                RetryPolicyV1::no_retry(),
            ),
            StepPlanV1::canonical_command(
                id("stp_second")?,
                ContentDigest([19; 32]),
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
    let tenant = id::<TenantId>("tnt_graph_benchmark")?;
    let process = id::<ProcessId>("prc_graph_benchmark")?;
    let iterations = 100_000_u32;
    let began = Instant::now();
    for _ in 0..iterations {
        let started = start_graph(
            black_box(&definition),
            tenant.clone(),
            process.clone(),
            GraphSagaInputV1::Start {
                input_id: id::<InputId>("inp_graph_start")?,
                action_id: id::<ActionId>("act_graph_first")?,
            },
        )?;
        let first = started
            .next_action
            .as_ref()
            .ok_or(BenchmarkError::MissingAction)?;
        let advanced = apply_graph_result(
            black_box(&definition),
            black_box(&started.projection),
            GraphSagaInputV1::ActionResult {
                input_id: id::<InputId>("inp_graph_result_one")?,
                observation: penelope_executor::engine::ActionResultObservationV1::succeeded(
                    first.action_id.clone(),
                ),
                next_action_id: Some(id::<ActionId>("act_graph_second")?),
            },
        )?;
        let second = advanced
            .next_action
            .as_ref()
            .ok_or(BenchmarkError::MissingAction)?;
        let completed = apply_graph_result(
            black_box(&definition),
            black_box(&advanced.projection),
            GraphSagaInputV1::ActionResult {
                input_id: id::<InputId>("inp_graph_result_two")?,
                observation: penelope_executor::engine::ActionResultObservationV1::succeeded(
                    second.action_id.clone(),
                ),
                next_action_id: None,
            },
        )?;
        black_box(completed);
    }
    let elapsed = began.elapsed();
    let nanos = elapsed.as_nanos() / u128::from(iterations);
    let ops = f64::from(iterations) / elapsed.as_secs_f64();
    println!(
        "graph-saga start/result/result: {iterations} operations in {elapsed:?} ({nanos} ns/op, {ops:.0} ops/s)"
    );
    Ok(())
}
