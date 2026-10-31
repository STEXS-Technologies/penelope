//! Cross-scope rejection stays atomic, with the same error precedence.
#![allow(clippy::unwrap_used)]
use penelope_domain::*;
use penelope_ports::{OutcomeLog, OutcomeLogValidationError};
enum Component {
    Tenant,
    Process,
    Definition,
    Version,
    Digest,
}
fn outcome(scope: &ProcessScope, sequence: u64) -> ProcessOutcome {
    ProcessOutcome::new(
        scope.clone(),
        sequence,
        ProcessOutcomeFact::new(
            format!("out_{sequence:04}").parse().unwrap(),
            CausationId::Input("inp_test".parse().unwrap()),
            OutcomeActor::System,
            LogicalTime(sequence),
            ProcessOutcomeKind::ActionPlanned,
            ContentDigest([2; 32]),
        ),
    )
}
#[test]
fn every_scope_component_is_checked_before_sequence_without_mutating_the_log() {
    for length in [5_usize, 22, 23, 128] {
        let suffix = "x".repeat(length.saturating_sub(4));
        let scope = ProcessScope::new(
            format!("tnt_{suffix}").parse().unwrap(),
            format!("prc_{suffix}").parse().unwrap(),
            format!("def_{suffix}").parse().unwrap(),
            format!("dfv_{suffix}").parse().unwrap(),
            ContentDigest([9; 32]),
        );
        for bad_rank in [0, 1] {
            for component in [
                Component::Tenant,
                Component::Process,
                Component::Definition,
                Component::Version,
                Component::Digest,
            ] {
                let mut log =
                    OutcomeLog::from_ordered(scope.clone(), &[outcome(&scope, 0)]).unwrap();
                let before = log.clone();
                let good = [outcome(&scope, 1), outcome(&scope, 2)];
                let mut bad = good.clone();
                let changed = bad.get_mut(bad_rank).unwrap();
                changed.sequence = 999;
                match component {
                    Component::Tenant => changed.tenant_id = "tnt_other".parse().unwrap(),
                    Component::Process => changed.process_id = "prc_other".parse().unwrap(),
                    Component::Definition => changed.definition_id = "def_other".parse().unwrap(),
                    Component::Version => changed.definition_version = "dfv_other".parse().unwrap(),
                    Component::Digest => changed.definition_digest = ContentDigest([17; 32]),
                }
                assert_eq!(
                    log.append(&bad),
                    Err(OutcomeLogValidationError::ScopeMismatch)
                );
                assert_eq!(log, before);
                log.append(&good).unwrap();
                let mut restored: OutcomeLog =
                    serde_json::from_slice(&serde_json::to_vec(&log).unwrap()).unwrap();
                assert_eq!(restored, log);
                let mut duplicate = outcome(&scope, 3);
                duplicate.outcome_id = good.first().unwrap().outcome_id.clone();
                assert_eq!(
                    restored.append(&[duplicate]),
                    Err(OutcomeLogValidationError::DuplicateOutcomeId)
                );
                assert_eq!(restored, log);
            }
        }
    }
}
