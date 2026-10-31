//! Differential checks against the pre-optimization public validation contract.
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
const fn random(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 32
}
#[test]
fn validation_preserves_all_effect_coordinates_and_error_precedence() {
    let base = fixture::commit(2);
    // All nine semantic coordinates must still distinguish different effects.
    // Action identity is deliberately excluded from EffectKey.
    for coordinate in 0..10 {
        let mut batch = base.clone();
        batch.actions[1] = batch.actions[0].clone();
        batch.actions[1].action_id = "act_other".parse().unwrap();
        match coordinate {
            0 => batch.actions[1].tenant_id = "tnt_other".parse().unwrap(),
            1 => batch.actions[1].process_id = "prc_other".parse().unwrap(),
            2 => batch.actions[1].definition_id = "def_other".parse().unwrap(),
            3 => batch.actions[1].definition_version = "dfv_other".parse().unwrap(),
            4 => batch.actions[1].definition_digest = ContentDigest([5; 32]),
            5 => batch.actions[1].step_id = "stp_other".parse().unwrap(),
            6 => batch.actions[1].attempt = 1,
            7 => batch.actions[1].kind = ProcessActionKind::Timer,
            8 => batch.actions[1].payload_digest = ContentDigest([5; 32]),
            _ => {}
        }
        assert_eq!(
            batch.validate(),
            legacy::validate(&batch),
            "coordinate {coordinate}"
        );
        assert_eq!(
            batch.actions[0].effect_key() == batch.actions[1].effect_key(),
            coordinate == 9
        );
        if coordinate == 9 {
            assert_eq!(
                batch.validate(),
                Err(CommitValidationError::DuplicateActionEffectKey)
            );
        }
    }
    let mut state = 42;
    for case in 0..10_000 {
        let count = if case % 64 == 0 { 128 } else { case % 17 };
        let mut batch = fixture::commit_with_identifiers(count, case % 2 != 0);
        for _ in 0..(random(&mut state) % 5) {
            let fault = random(&mut state) % 20;
            let index = if count == 0 {
                0
            } else {
                usize::try_from(random(&mut state)).unwrap() % count
            };
            match fault {
                0 if count > 1 => {
                    batch.actions[index].action_id =
                        batch.actions[(index + 1) % count].action_id.clone()
                }
                1 if count > 1 => {
                    let identity = batch.actions[index].action_id.clone();
                    batch.actions[index] = batch.actions[(index + 1) % count].clone();
                    batch.actions[index].action_id = identity;
                }
                2 if count > 0 => batch.actions[index].tenant_id = "tnt_other".parse().unwrap(),
                3 if count > 0 => batch.actions[index].process_id = "prc_other".parse().unwrap(),
                4 if count > 0 => batch.actions[index].definition_id = "def_other".parse().unwrap(),
                5 if count > 0 => {
                    batch.actions[index].definition_version = "dfv_other".parse().unwrap()
                }
                6 if count > 0 => batch.actions[index].definition_digest = ContentDigest([5; 32]),
                7 if count > 0 => batch.actions[index].step_id = "stp_other".parse().unwrap(),
                8 if count > 0 => batch.actions[index].attempt = u32::MAX,
                9 if count > 0 => batch.actions[index].kind = ProcessActionKind::Timer,
                10 if count > 0 => batch.actions[index].payload_digest = ContentDigest([5; 32]),
                11 => batch.expected_sequence = u64::MAX,
                12 => batch.outcomes[0].tenant_id = "tnt_other".parse().unwrap(),
                13 => batch.outcomes[0].sequence = 1,
                14 => batch.input = None,
                15 => batch.canonical_source_event_id = Some("cev_source".parse().unwrap()),
                16 => {
                    if let Some(input) = &mut batch.input {
                        input.tenant_id = "tnt_other".parse().unwrap();
                    }
                }
                17 => {
                    if let Some(input) = &mut batch.input {
                        input.kind = ProcessInputKind::TimerFired;
                    }
                }
                18 => {
                    batch.outcomes[0].causation_id =
                        CausationId::Action("act_other".parse().unwrap())
                }
                _ => {}
            }
        }
        assert_eq!(batch.validate(), legacy::validate(&batch), "case {case}");
    }
}

#[test]
fn shared_step_payloads_and_later_invalid_scopes_preserve_effect_identity() {
    for long in [false, true] {
        let mut batch = fixture::commit_with_identifiers(128, long);
        fixture::vary_payloads(&mut batch);
        assert_eq!(batch.validate(), Ok(()));
        assert_eq!(batch.validate(), legacy::validate(&batch));
        batch.actions[127].payload_digest = batch.actions[0].payload_digest;
        assert_eq!(
            batch.validate(),
            Err(CommitValidationError::DuplicateActionEffectKey)
        );
        assert_eq!(batch.validate(), legacy::validate(&batch));
        for coordinate in 0..5 {
            let mut invalid_scope = batch.clone();
            match coordinate {
                0 => invalid_scope.actions[127].tenant_id = "tnt_other".parse().unwrap(),
                1 => invalid_scope.actions[127].process_id = "prc_other".parse().unwrap(),
                2 => invalid_scope.actions[127].definition_id = "def_other".parse().unwrap(),
                3 => invalid_scope.actions[127].definition_version = "dfv_other".parse().unwrap(),
                _ => invalid_scope.actions[127].definition_digest = ContentDigest([5; 32]),
            }
            assert_eq!(
                invalid_scope.validate(),
                Err(CommitValidationError::ActionScopeMismatch)
            );
            assert_eq!(invalid_scope.validate(), legacy::validate(&invalid_scope));
        }
    }
}
