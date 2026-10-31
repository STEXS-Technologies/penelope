//! Borrowed validation must preserve every component of the owned-scope check.
#![allow(clippy::unwrap_used)]
use penelope_domain::*;

#[test]
fn scope_validation_matches_owned_comparison_at_identifier_boundaries() {
    for length in [5_usize, 22, 23, 24, 25, 128] {
        let suffix = "x".repeat(length.saturating_sub(4));
        let scope = ProcessScope::new(
            format!("tnt_{suffix}").parse().unwrap(),
            format!("prc_{suffix}").parse().unwrap(),
            format!("def_{suffix}").parse().unwrap(),
            format!("dfv_{suffix}").parse().unwrap(),
            ContentDigest([13; 32]),
        );
        let outcome = ProcessOutcome::new(
            scope.clone(),
            7,
            ProcessOutcomeFact::new(
                "out_test".parse().unwrap(),
                CausationId::Input("inp_test".parse().unwrap()),
                OutcomeActor::System,
                LogicalTime(1),
                ProcessOutcomeKind::Started,
                ContentDigest([42; 32]),
            ),
        );
        // All combinations of substitutions, including each component alone.
        for mask in 0_u8..32 {
            let mut expected = scope.clone();
            if mask & 1 != 0 {
                expected.tenant_id = "tnt_other".parse().unwrap();
            }
            if mask & 2 != 0 {
                expected.process_id = "prc_other".parse().unwrap();
            }
            if mask & 4 != 0 {
                expected.definition_id = "def_other".parse().unwrap();
            }
            if mask & 8 != 0 {
                expected.definition_version = "dfv_other".parse().unwrap();
            }
            if mask & 16 != 0 {
                expected.definition_digest = ContentDigest([14; 32]);
            }
            let owned = if outcome.scope() == expected {
                Ok(())
            } else {
                Err(DomainError::OutcomeScopeMismatch)
            };
            assert_eq!(outcome.validate_for_scope(&expected), owned);
            assert_eq!(owned.is_ok(), mask == 0);
        }
    }
}
