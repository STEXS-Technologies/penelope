//! Direct comparison must retain all five scope coordinates.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use super::*;
#[path = "../../penelope-ports/tests/support/timer_claim_fixture.rs"]
mod fixture;
#[test]
fn borrowed_claim_scope_matches_owned_equality() {
    let mut cases = 0;
    for length in [5, 22, 23, 128] {
        for mask in 0..32 {
            let (lease, mut request) = fixture::fixture(length);
            if mask & 1 != 0 {
                request.scope.tenant_id = "tnt_other".parse().unwrap();
            }
            if mask & 2 != 0 {
                request.scope.process_id = "prc_other".parse().unwrap();
            }
            if mask & 4 != 0 {
                request.scope.definition_id = "def_other".parse().unwrap();
            }
            if mask & 8 != 0 {
                request.scope.definition_version = "dfv_other".parse().unwrap();
            }
            if mask & 16 != 0 {
                request.scope.definition_digest = penelope_domain::ContentDigest([8; 32]);
            }
            assert_eq!(
                action_in_scope(&lease.timer.action, &request.scope),
                lease.timer.action.scope() == request.scope
            );
            cases += 1;
        }
    }
    for (_profile, lease, request) in fixture::profiles() {
        assert_eq!(
            action_in_scope(&lease.timer.action, &request.scope),
            lease.timer.action.scope() == request.scope
        );
    }
    assert_eq!(cases, 128);
    println!("borrowed_claim_scope_cases={cases} fixture_profiles=32 all_coordinates=true");
}
