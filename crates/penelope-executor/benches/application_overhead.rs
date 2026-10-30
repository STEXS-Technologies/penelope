//! Paired direct versus in-memory orchestration of the identical CPU task.
//! No journal, dispatch port, SQLite, isolation, or crash recovery is exercised.

#![allow(clippy::arithmetic_side_effects, clippy::float_arithmetic)]

use std::{hint::black_box, time::Instant};

use penelope_domain::{ActionId, ContentDigest, DomainError, ProcessId, TenantId};
use penelope_executor::engine::{
    ActionResultObservation, EngineError, LinearSagaDefinition, RetryPolicy, SagaStatus, StepPlan,
    apply_action_result, start,
};

// Keep precisely the same task call in both paths, including runtime input.
#[inline(never)]
fn task(mut value: u64, work: u32) -> u64 {
    for _ in 0..work {
        value = value
            .wrapping_mul(6_364_136_223_846_793_005)
            .rotate_left(17)
            .wrapping_add(1_442_695_040_888_963_407);
    }
    value
}

fn id<T: TryFrom<&'static str, Error = DomainError>>(
    value: &'static str,
) -> Result<T, DomainError> {
    T::try_from(value)
}

fn measure(
    mut operation: impl FnMut(u64) -> Result<u64, EngineError>,
    iterations: u32,
) -> Result<(u128, u64), EngineError> {
    let began = Instant::now();
    let mut checksum = 0;
    for index in 0..iterations {
        checksum ^= black_box(operation(black_box(u64::from(index)))?);
    }
    Ok((began.elapsed().as_nanos(), checksum))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let definition = LinearSagaDefinition {
        definition_id: id("def_overhead")?,
        definition_version: id("dfv_one")?,
        definition_digest: ContentDigest([99; 32]),
        steps: vec![StepPlan::canonical_command(
            id("stp_overhead")?,
            ContentDigest([1; 32]),
            RetryPolicy::no_retry(),
        )],
    };
    let tenant: TenantId = id("tnt_overhead")?;
    let process: ProcessId = id("prc_overhead")?;
    let action: ActionId = id("act_overhead")?;
    for (work, iterations) in [(0, 100_000), (64, 100_000), (4096, 2000), (65_536, 128)] {
        let direct = |seed| Ok::<_, EngineError>(task(seed, black_box(work)));
        let managed = |seed| {
            let started = start(
                black_box(&definition),
                tenant.clone(),
                process.clone(),
                action.clone(),
            )?;
            let result = task(seed, black_box(work));
            let completed = apply_action_result(
                black_box(&definition),
                black_box(&started.projection),
                tenant.clone(),
                process.clone(),
                &ActionResultObservation::succeeded(action.clone()),
                None,
            )?;
            let completed = black_box(completed);
            if completed.projection.status != SagaStatus::Completed
                || completed.next_action.is_some()
            {
                return Err(EngineError::InvalidProjection);
            }
            Ok(result)
        };
        // Untimed warmups and semantic checks for every distinct work size.
        for seed in 0..100 {
            if direct(seed)? != managed(seed)? {
                return Err("task outputs differ".into());
            }
        }
        for round in 0..40 {
            let (direct_result, managed_result) = if round % 2 == 0 {
                (measure(direct, iterations)?, measure(managed, iterations)?)
            } else {
                let managed_result = measure(managed, iterations)?;
                (measure(direct, iterations)?, managed_result)
            };
            if direct_result.1 != managed_result.1 {
                return Err("checksums differ".into());
            }
            println!(
                "{}",
                serde_json::json!({
                    "work": work, "round": round, "iterations": iterations,
                    "direct_ns": direct_result.0, "penelope_ns": managed_result.0,
                    "checksum": direct_result.1,
                })
            );
        }
    }
    Ok(())
}
