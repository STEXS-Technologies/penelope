//! Check the reference workload's semantic contract independently of timing.
#![allow(clippy::arithmetic_side_effects)]
#[path = "../benches/support/application_workloads/workflow.rs"]
mod workload;
use workload::{Admission, Case, Error, Fixture, kernel};
#[test]
fn all_workflow_paths_match_identical_business_work() -> Result<(), Error> {
    for admission in [Admission::Preaccepted, Admission::Atomic] {
        if Admission::parse(admission.name())? != admission {
            return Err("admission roundtrip differs".into());
        }
        for case in Case::ALL {
            if Case::parse(case.name())? != case {
                return Err("case roundtrip differs".into());
            }
            let mut fixture = if admission == Admission::Preaccepted {
                Fixture::new(case)?
            } else {
                Fixture::with_admission(case, admission)?
            };
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
    }
    Ok(())
}
