//! Valid effect identities shared by the parity corpus and isolated probes.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use penelope_domain::*;
pub const PROFILES: [(&str, [usize; 5]); 5] = [
    ("short", [9; 5]),
    ("inline-limit", [22; 5]),
    ("heap-first", [23; 5]),
    ("maximum", [128; 5]),
    ("mixed", [5, 22, 23, 64, 128]),
];
pub const KINDS: [ProcessActionKind; 4] = [
    ProcessActionKind::CanonicalCommand,
    ProcessActionKind::Timer,
    ProcessActionKind::ExternalEffect,
    ProcessActionKind::ManualReview,
];
pub fn key(lengths: [usize; 5], kind: ProcessActionKind, attempt: u32, payload: u8) -> EffectKey {
    let [tenant, process, definition, version, step] = lengths;
    EffectKey {
        tenant_id: format!("tnt_{}", "x".repeat(tenant - 4)).parse().unwrap(),
        process_id: format!("prc_{}", "x".repeat(process - 4)).parse().unwrap(),
        definition_id: format!("def_{}", "x".repeat(definition - 4))
            .parse()
            .unwrap(),
        definition_version: format!("dfv_{}", "x".repeat(version - 4)).parse().unwrap(),
        definition_digest: ContentDigest([payload.wrapping_add(1); 32]),
        step_id: format!("stp_{}", "x".repeat(step - 4)).parse().unwrap(),
        attempt,
        kind,
        payload_digest: ContentDigest([payload; 32]),
    }
}
