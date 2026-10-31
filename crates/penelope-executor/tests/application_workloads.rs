//! Check the reference workload's semantic contract independently of timing.
#![allow(clippy::arithmetic_side_effects)]
#[path = "../benches/support/application_workloads/workflow.rs"]
mod workload;
use workload::{Case, Error, Fixture, kernel};
#[test]
fn all_workflow_paths_match_identical_business_work() -> Result<(), Error> {
    for case in Case::ALL {
        if Case::parse(case.name())? != case {
            return Err("case roundtrip differs".into());
        }
        let mut fixture = Fixture::new(case)?;
        for seed in 0..4 {
            let mut expected = seed;
            for _ in 0..case.calls() {
                expected = kernel(expected, 64);
            }
            if fixture.run(case, seed, 64)? != expected {
                return Err("business output differs".into());
            }
        }
    }
    Ok(())
}
