//! Reproducible randomized byte traces plus exhaustive short transition sequences.
#[path = "support/timeline_model.rs"]
mod model;
use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::SourceParallel("proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    #[test]
    fn metadata_state_machine(trace in prop::collection::vec(any::<u8>(),0..256)) { model::projection(&trace); }
    #[test]
    fn immutable_plan_admission(trace in prop::collection::vec(any::<u8>(),0..256)) { model::plans(&trace); }
    #[test]
    fn correction_matches_closed_form(trace in prop::collection::vec(any::<u8>(),0..256)) { model::correction(&trace); }
    #[test]
    fn promotion_interleavings(trace in prop::collection::vec(any::<u8>(),0..256)) { model::promotion(&trace); }
    #[test]
    fn finalized_partial_handoff(trace in prop::collection::vec(any::<u8>(),0..256)) { model::effects(&trace); }
}
#[test]
fn exhaustive_three_transition_metadata_traces() {
    // Each of 8 operations at low/high parameters, including empty/full slots and
    // stale candidates after append/observe/finality/restore: 16^3 short traces.
    for first in 0_u8..16 {
        for second in 0_u8..16 {
            for third in 0_u8..16 {
                model::projection(&[
                    first % 8,
                    first / 8 * 255,
                    second % 8,
                    second / 8 * 255,
                    third % 8,
                    third / 8 * 255,
                ]);
            }
        }
    }
}
#[test]
fn exhaustive_short_corrections_fail_at_every_replay_step() {
    for horizon in 0_u8..4 {
        for kind in 0_u8..2 {
            for corrected in 0_u8..=4 {
                for failure in 0_u8..6 {
                    for fault_step in 0_u8..=4 {
                        let mut trace = [0; 32];
                        trace[0] = horizon;
                        trace[2] = kind;
                        trace[3] = corrected;
                        trace[4] = 19;
                        trace[5] = failure;
                        trace[6] = fault_step;
                        model::correction(&trace);
                    }
                }
            }
        }
    }
}
