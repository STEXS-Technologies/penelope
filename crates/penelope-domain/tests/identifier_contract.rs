//! Frozen String-based admission, wire and trait contracts across storage sizes.
#![allow(clippy::unwrap_used)]
use penelope_domain::*;
use proptest::prelude::*;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

fn hash(value: &impl Hash) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

// Original identifier validation expression, independent of the new storage.
fn valid(prefix: &str, value: &str) -> bool {
    value.starts_with(prefix)
        && value.len() > prefix.len()
        && value.len() <= 128
        && value.get(prefix.len()..).is_some_and(|body| {
            body.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
}

proptest! {
    #[test]
    fn public_identifiers_preserve_admission_wire_and_traits(
        body in "[a-zA-Z0-9_.-]{0,140}",
        other in "[a-zA-Z0-9_.-]{1,100}",
        arbitrary in any::<String>(),
    ) {
        macro_rules! check {
            ($kind:ty, $prefix:literal, $error:ident) => {{
                for text in [format!("{}{}", $prefix, body), arbitrary.clone()] {
                    let parsed = text.parse::<$kind>();
                    prop_assert_eq!(parsed.is_ok(), valid($prefix, &text));
                    prop_assert_eq!(&parsed, &<$kind>::new(text.clone()));
                    prop_assert_eq!(&parsed, &<$kind>::try_from(text.as_str()));
                    prop_assert_eq!(&parsed, &<$kind>::try_from(text.clone()));
                    let json = serde_json::to_string(&text).unwrap();
                    let decoded = serde_json::from_str::<$kind>(&json);
                    prop_assert_eq!(decoded.is_ok(), parsed.is_ok());
                    if let Ok(value) = parsed {
                        prop_assert_eq!(value.as_str(), text.as_str());
                        prop_assert_eq!(value.to_string(), text.as_str());
                        prop_assert_eq!(format!("{value:?}"), format!("{}({text:?})", stringify!($kind)));
                        prop_assert_eq!(serde_json::to_string(&value).unwrap(), json);
                        prop_assert_eq!(decoded.unwrap(), value.clone());
                        prop_assert_eq!(hash(&value), hash(&text));
                        let other_text = format!("{}{}", $prefix, other);
                        let other_value: $kind = other_text.parse().unwrap();
                        prop_assert_eq!(value.cmp(&other_value), text.cmp(&other_text));
                        prop_assert_eq!(value == other_value, text == other_text);
                    } else {
                        prop_assert_eq!(parsed, Err(DomainError::$error));
                    }
                }
            }};
        }
        check!(TenantId, "tnt_", InvalidTenantId);
        check!(ProcessId, "prc_", InvalidProcessId);
        check!(DefinitionId, "def_", InvalidDefinitionId);
        check!(DefinitionVersion, "dfv_", InvalidDefinitionVersion);
        check!(StepId, "stp_", InvalidStepId);
        check!(InputId, "inp_", InvalidInputId);
        check!(OutcomeId, "out_", InvalidOutcomeId);
        check!(ActionId, "act_", InvalidActionId);
        check!(ReviewId, "rev_", InvalidReviewId);
        check!(PrincipalId, "pri_", InvalidPrincipalId);
        check!(CanonicalEventId, "cev_", InvalidCanonicalEventId);
        check!(CanonicalCommitId, "cmt_", InvalidCanonicalCommitId);
        check!(ResourceId, "res_", InvalidResourceId);
        check!(OperationId, "op_", InvalidOperationId);
        check!(ExternalReferenceId, "ext_", InvalidExternalReferenceId);
        check!(MigrationId, "mig_", InvalidMigrationId);
    }
}
