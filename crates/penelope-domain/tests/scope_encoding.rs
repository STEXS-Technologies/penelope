//! Canonical identity bytes must remain stable across allocation changes.
#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
use penelope_domain::*;

#[test]
fn canonical_scope_matches_legacy_encoding_across_identifier_boundaries() {
    let varied = (0_usize..10000).map(|case| {
        [
            5 + case % 124,
            5 + case / 3 % 124,
            5 + case / 7 % 124,
            5 + case / 11 % 124,
        ]
    });
    for (case, lengths) in varied
        .chain([[5; 4], [23; 4], [24; 4], [25; 4], [128; 4]])
        .enumerate()
    {
        let identifiers: Vec<String> = ["tnt_", "prc_", "def_", "dfv_"]
            .into_iter()
            .zip(lengths)
            .map(|(prefix, length)| format!("{prefix}{}", "x".repeat(length - prefix.len())))
            .collect();
        let digest_byte = u8::try_from(case % 256).unwrap();
        let scope = ProcessScope::new(
            identifiers[0].parse().unwrap(),
            identifiers[1].parse().unwrap(),
            identifiers[2].parse().unwrap(),
            identifiers[3].parse().unwrap(),
            ContentDigest([digest_byte; 32]),
        );
        let mut legacy = Vec::new();
        for identifier in &identifiers {
            legacy.extend_from_slice(&u16::try_from(identifier.len()).unwrap().to_be_bytes());
            legacy.extend_from_slice(identifier.as_bytes());
        }
        legacy.extend_from_slice(&[digest_byte; 32]);
        let encoded = scope.canonical_bytes();
        assert_eq!(encoded, legacy, "case={case}");
        assert_eq!(encoded.len(), 40 + lengths.into_iter().sum::<usize>());
    }
}
