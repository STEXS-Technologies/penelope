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
                    log.append(std::slice::from_ref(changed)),
                    Err(OutcomeLogValidationError::ScopeMismatch)
                );
                assert_eq!(log, before);
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

#[test]
fn single_and_batch_appends_share_duplicate_tracking_through_the_replay_bound() {
    let scope = ProcessScope::new(
        "tnt_stream".parse().unwrap(),
        "prc_stream".parse().unwrap(),
        "def_stream".parse().unwrap(),
        "dfv_stream".parse().unwrap(),
        ContentDigest([9; 32]),
    );
    let history: Vec<_> = (0..penelope_ports::MAX_OUTCOMES_PER_REPLAY_LOG)
        .map(|sequence| outcome(&scope, u64::try_from(sequence).unwrap()))
        .collect();
    let mut log = OutcomeLog::new(scope.clone());
    for (rank, batch) in history.chunks(3).enumerate() {
        // Alternate single and multi-item appends against one shared ID index.
        log.append(batch.get(..1).unwrap()).unwrap();
        if [0, 1, 42, 1364].contains(&rank) {
            let before = log.clone();
            let mut duplicate = outcome(&scope, log.next_sequence());
            duplicate.outcome_id = batch.first().unwrap().outcome_id.clone();
            assert_eq!(
                log.append(&[duplicate]),
                Err(OutcomeLogValidationError::DuplicateOutcomeId)
            );
            assert_eq!(log, before);
            let mut invalid_batch = batch.get(1..).unwrap().to_vec();
            invalid_batch.last_mut().unwrap().outcome_id =
                batch.first().unwrap().outcome_id.clone();
            assert_eq!(
                log.append(&invalid_batch),
                Err(OutcomeLogValidationError::DuplicateOutcomeId)
            );
            assert_eq!(log, before);
        }
        log.append(batch.get(1..).unwrap()).unwrap();
        if [0, 1, 42, 1364].contains(&rank) {
            let before = log.clone();
            let mut duplicate = outcome(&scope, log.next_sequence());
            duplicate.outcome_id = batch.last().unwrap().outcome_id.clone();
            assert_eq!(
                log.append(&[duplicate]),
                Err(OutcomeLogValidationError::DuplicateOutcomeId)
            );
            assert_eq!(log, before);
            log = serde_json::from_slice(&serde_json::to_vec(&log).unwrap()).unwrap();
            assert_eq!(log, before);
        }
    }
    assert_eq!(log.outcomes(), history);
    let rebuilt = OutcomeLog::from_ordered(scope.clone(), &history).unwrap();
    assert_eq!(log, rebuilt);
    assert_eq!(log.append(&[]), Ok(()));
    assert_eq!(log, rebuilt);
    assert_eq!(
        log.append(&[outcome(&scope, log.next_sequence())]),
        Err(OutcomeLogValidationError::LimitExceeded)
    );
    assert_eq!(log, rebuilt);
}
