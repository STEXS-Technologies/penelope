//! Direct encoding binds every semantic coordinate at integer/digest boundaries.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
#[path = "support/action_effect_fixture.rs"]
mod action_fixture;
#[path = "support/effect_key_fixture.rs"]
mod fixture;
use penelope_domain::*;
#[test]
fn direct_action_encoding_preserves_boundaries_and_all_semantic_coordinates() {
    let mut cases = 0;
    for (_profile, lengths) in fixture::PROFILES {
        for kind in fixture::KINDS {
            for attempt in [0, 1, u32::MAX - 1, u32::MAX] {
                for payload in [0, 255] {
                    let key = fixture::key(lengths, kind, attempt, payload);
                    let action = action_fixture::action(key.clone());
                    let expected = key.canonical_bytes();
                    assert_eq!(action.canonical_effect_key_bytes(), expected);
                    for coordinate in 0..9 {
                        let mut changed = action.clone();
                        match coordinate {
                            0 => changed.tenant_id = "tnt_other".parse().unwrap(),
                            1 => changed.process_id = "prc_other".parse().unwrap(),
                            2 => changed.definition_id = "def_other".parse().unwrap(),
                            3 => changed.definition_version = "dfv_other".parse().unwrap(),
                            4 => changed.definition_digest.0[0] ^= 1,
                            5 => changed.step_id = "stp_other".parse().unwrap(),
                            6 => changed.attempt ^= 1,
                            7 => {
                                changed.kind = if kind == ProcessActionKind::Timer {
                                    ProcessActionKind::ExternalEffect
                                } else {
                                    ProcessActionKind::Timer
                                }
                            }
                            _ => changed.payload_digest.0[0] ^= 1,
                        }
                        assert_ne!(
                            changed.canonical_effect_key_bytes(),
                            expected,
                            "coordinate={coordinate}"
                        );
                        assert_eq!(
                            changed.canonical_effect_key_bytes(),
                            changed.effect_key().canonical_bytes()
                        );
                    }
                    let mut renamed = action;
                    renamed.action_id = "act_other".parse().unwrap();
                    assert_eq!(renamed.canonical_effect_key_bytes(), expected);
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 160);
    println!(
        "action_effect_boundary_cases={cases} coordinate_mutations=1440 zero_attempt=true max_attempt=true action_id_excluded=true"
    );
}
