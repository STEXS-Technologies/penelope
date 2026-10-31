//! Isolated linear-definition admission cost; excludes saga transitions and I/O.
use penelope_domain::{ContentDigest, StepId};
use penelope_executor::engine::{LinearSagaDefinition, RetryPolicy, StepPlan};
use std::{hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for long in [false, true] {
        let prefix = if long { "x".repeat(96) } else { "x".into() };
        for count in [1_u32, 2, 4, 8, 9, 16, 17, 32, 64, 128] {
            for order in ["ascending", "descending", "permuted"] {
                let mut keys: Vec<_> = (0..count).collect();
                if order == "descending" {
                    keys.reverse();
                }
                if order == "permuted" {
                    // A fixed permutation; fixture construction is untimed.
                    for index in 0..keys.len() {
                        let other = index
                            .saturating_mul(37)
                            .saturating_add(11)
                            .checked_rem(keys.len())
                            .ok_or("empty fixture")?;
                        keys.swap(index, other);
                    }
                }
                let steps = keys
                    .into_iter()
                    .map(|key| {
                        Ok(StepPlan::canonical_command(
                            format!("stp_{prefix}_{key:03}").parse::<StepId>()?,
                            ContentDigest([1; 32]),
                            RetryPolicy::no_retry(),
                        ))
                    })
                    .collect::<Result<Vec<_>, penelope_domain::DomainError>>()?;
                let definition = LinearSagaDefinition {
                    definition_id: "def_validation".parse()?,
                    definition_version: "dfv_one".parse()?,
                    definition_digest: ContentDigest([2; 32]),
                    steps,
                };
                definition.validate()?;
                let iterations = if count <= 17 { 100_000_u32 } else { 10_000_u32 };
                let began = Instant::now();
                for _ in 0..iterations {
                    black_box(black_box(&definition).validate())?;
                }
                let elapsed = began.elapsed();
                let nanos = elapsed
                    .as_nanos()
                    .checked_div(u128::from(iterations))
                    .ok_or("invalid iterations")?;
                println!(
                    "linear definition count={count} long={long} order={order}: {iterations} operations in {elapsed:?} ({nanos} ns/op)"
                );
            }
        }
    }
    Ok(())
}
