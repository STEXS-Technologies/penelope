//! Consumer-facing witness for the original hosted error path and variant imports.
use penelope_core::task::{TaskError, runtime::TaskPortError};
use penelope_executor::task::SupervisorError;
use penelope_executor::task::SupervisorError::{Decision, Port, ReconstructionRequired};

const fn assert_public_traits<T: std::error::Error + Send + Sync + Clone + Copy + Eq>() {}

const fn classify(error: SupervisorError) -> u8 {
    match error {
        Decision(_) => 0,
        Port(_) => 1,
        ReconstructionRequired => 2,
    }
}

#[test]
fn hosted_error_path_variant_imports_conversions_and_exhaustive_match_still_compile() {
    assert_public_traits::<SupervisorError>();
    assert_eq!(classify(TaskError::InvalidOrder.into()), 0);
    assert_eq!(classify(TaskPortError::Unavailable.into()), 1);
    assert_eq!(classify(ReconstructionRequired), 2);
}
