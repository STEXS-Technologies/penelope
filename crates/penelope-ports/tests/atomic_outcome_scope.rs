//! Exact scope-error precedence and legacy parity in multi-row commits.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use penelope_domain::*;
use penelope_ports::*;
#[path = "support/atomic_validation_fixture.rs"]
mod fixture;
#[path = "support/legacy_atomic_validator.rs"]
mod legacy;
fn batch(length: usize) -> AtomicProcessCommit {
    let suffix = "x".repeat(length - 4);
    let scope = ProcessScope::new(
        format!("tnt_{suffix}").parse().unwrap(),
        format!("prc_{suffix}").parse().unwrap(),
        format!("def_{suffix}").parse().unwrap(),
        format!("dfv_{suffix}").parse().unwrap(),
        ContentDigest([9; 32]),
    );
    let mut committed = fixture::commit(2);
    fixture::vary_payloads(&mut committed);
    let input = committed.input.as_mut().unwrap();
    input.tenant_id = scope.tenant_id.clone();
    input.process_id = scope.process_id.clone();
    let input_id = input.input_id.clone();
    committed.outcomes = (0..MAX_OUTCOMES_PER_ATOMIC_COMMIT)
        .map(|sequence| {
            ProcessOutcome::new(
                scope.clone(),
                u64::try_from(sequence).unwrap(),
                ProcessOutcomeFact::new(
                    format!("out_scope_{sequence:04}").parse().unwrap(),
                    if sequence == 0 {
                        CausationId::Input(input_id.clone())
                    } else {
                        CausationId::Action("act_cause".parse().unwrap())
                    },
                    if sequence % 2 == 0 {
                        OutcomeActor::System
                    } else {
                        OutcomeActor::Principal("pri_scope".parse().unwrap())
                    },
                    LogicalTime(u64::try_from(sequence).unwrap()),
                    if sequence == 0 {
                        ProcessOutcomeKind::Started
                    } else {
                        ProcessOutcomeKind::ActionPlanned
                    },
                    ContentDigest([u8::try_from(sequence).unwrap(); 32]),
                ),
            )
        })
        .collect();
    for action in &mut committed.actions {
        action.tenant_id = scope.tenant_id.clone();
        action.process_id = scope.process_id.clone();
        action.definition_id = scope.definition_id.clone();
        action.definition_version = scope.definition_version.clone();
        action.definition_digest = scope.definition_digest;
    }
    committed
}
#[test]
fn multi_outcome_scope_masks_preserve_exact_error_precedence_and_ignore_event_fields() {
    let mut cases = 0;
    for length in [5, 22, 23, 128] {
        let valid = batch(length);
        assert_eq!(valid.validate(), Ok(()));
        assert_eq!(legacy::validate(&valid), Ok(()));
        for rank in [0, 1, 127] {
            for mask in 0..32 {
                for competing in 0..4 {
                    let mut changed = valid.clone();
                    let row = &mut changed.outcomes[rank];
                    if mask & 1 != 0 {
                        row.tenant_id = "tnt_other".parse().unwrap();
                    }
                    if mask & 2 != 0 {
                        row.process_id = "prc_other".parse().unwrap();
                    }
                    if mask & 4 != 0 {
                        row.definition_id = "def_other".parse().unwrap();
                    }
                    if mask & 8 != 0 {
                        row.definition_version = "dfv_other".parse().unwrap();
                    }
                    if mask & 16 != 0 {
                        row.definition_digest = ContentDigest([8; 32]);
                    }
                    let expected = match competing {
                        1 => {
                            changed.outcomes[0].sequence = 1;
                            Err(CommitValidationError::NonContiguousSequence)
                        }
                        2 => {
                            changed.outcomes[127].outcome_id =
                                changed.outcomes[0].outcome_id.clone();
                            Err(CommitValidationError::DuplicateOutcomeId)
                        }
                        3 => {
                            changed.actions[0].tenant_id = "tnt_bad_action".parse().unwrap();
                            if mask == 0 {
                                Err(CommitValidationError::ActionScopeMismatch)
                            } else {
                                Err(CommitValidationError::OutcomeScopeMismatch)
                            }
                        }
                        _ => {
                            if mask == 0 {
                                Ok(())
                            } else {
                                Err(CommitValidationError::OutcomeScopeMismatch)
                            }
                        }
                    };
                    assert_eq!(
                        changed.validate(),
                        expected,
                        "length={length} rank={rank} mask={mask} competing={competing}"
                    );
                    assert_eq!(changed.validate(), legacy::validate(&changed));
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 1536);
    println!(
        "multi_outcome_cases={cases} exact_error_parity=true all_scope_masks=true event_fields_ignored=true"
    );
}
