//! Canonical effect identity remains byte-for-byte compatible with the old format.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
use penelope_domain::*;
#[path = "support/action_effect_fixture.rs"]
mod action_fixture;
#[path = "support/effect_key_fixture.rs"]
mod fixture;
fn legacy(key: &EffectKey) -> Vec<u8> {
    let mut bytes = Vec::new();
    for id in [
        key.tenant_id.as_str(),
        key.process_id.as_str(),
        key.definition_id.as_str(),
        key.definition_version.as_str(),
    ] {
        bytes.extend_from_slice(&u16::try_from(id.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(id.as_bytes());
    }
    bytes.extend_from_slice(&key.definition_digest.0);
    let step = key.step_id.as_str();
    bytes.extend_from_slice(&u16::try_from(step.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(step.as_bytes());
    bytes.extend_from_slice(&key.attempt.to_be_bytes());
    bytes.push(match key.kind {
        ProcessActionKind::CanonicalCommand => 0,
        ProcessActionKind::Timer => 1,
        ProcessActionKind::ExternalEffect => 2,
        ProcessActionKind::ManualReview => 3,
    });
    bytes.extend_from_slice(&key.payload_digest.0);
    bytes
}
#[test]
fn canonical_effect_keys_match_legacy_across_sizes_kinds_attempts_and_digests() {
    let varied = (0_usize..10000).map(|case| {
        [
            5 + case % 124,
            5 + case / 3 % 124,
            5 + case / 7 % 124,
            5 + case / 11 % 124,
            5 + case / 13 % 124,
        ]
    });
    let mut cases = 0;
    for (case, lengths) in varied
        .chain(fixture::PROFILES.map(|(_name, lengths)| lengths))
        .enumerate()
    {
        let attempt = if case % 7 == 0 {
            u32::MAX
        } else {
            u32::try_from(case).unwrap().wrapping_mul(2654435761)
        };
        let key = fixture::key(
            lengths,
            fixture::KINDS[case % 4],
            attempt,
            u8::try_from(case % 256).unwrap(),
        );
        let encoded = key.canonical_bytes();
        assert_eq!(encoded, legacy(&key), "case={case}");
        let mut action = action_fixture::action(key.clone());
        assert_eq!(action.effect_key(), key);
        assert_eq!(
            action.canonical_effect_key_bytes(),
            encoded,
            "action case={case}"
        );
        action.action_id = "act_other".parse().unwrap();
        assert_eq!(
            action.canonical_effect_key_bytes(),
            encoded,
            "action ID case={case}"
        );
        assert_eq!(encoded.len(), 79 + lengths.into_iter().sum::<usize>());
        cases += 1;
    }
    assert_eq!(cases, 10005);
    println!(
        "effect_key_legacy_cases={cases} all_kinds=true attempt_boundaries=true exact_bytes=true direct_action=true action_id_excluded=true"
    );
}
