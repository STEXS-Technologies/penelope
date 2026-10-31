//! Pinned comparison preserves exact timer claim validation precedence.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use penelope_domain::*;
use penelope_ports::*;
use std::num::NonZeroU16;
#[path = "support/timer_claim_fixture.rs"]
mod fixture;
fn legacy(lease: &TimerLease, request: &TimerClaimRequest) -> Result<(), PortError> {
    request.validate()?;
    lease
        .timer
        .validate()
        .map_err(|_error| PortError::Invariant)?;
    if lease.timer.action.scope() != request.scope
        || lease.owner != request.owner
        || lease.timer.due_at > request.now
    {
        return Err(PortError::Invariant);
    }
    if request.now > lease.lease_expires_at {
        return Err(PortError::TimedOut);
    }
    Ok(())
}
#[test]
fn timer_validation_matches_legacy_for_competing_errors_and_boundaries() {
    let mut cases = 0;
    for length in [5, 22, 23, 128] {
        for mask in 0..32 {
            for other_owner in [false, true] {
                for due in [99, 100, 101] {
                    for expires in [99, 100, 101] {
                        for kind in [
                            ProcessActionKind::CanonicalCommand,
                            ProcessActionKind::Timer,
                            ProcessActionKind::ExternalEffect,
                            ProcessActionKind::ManualReview,
                        ] {
                            for oversized in [false, true] {
                                let (mut lease, mut request) = fixture::fixture(length);
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
                                    request.scope.definition_digest = ContentDigest([8; 32]);
                                }
                                if other_owner {
                                    request.owner = "pri_other".parse().unwrap();
                                }
                                lease.timer.due_at = LogicalTime(due);
                                lease.lease_expires_at = LogicalTime(expires);
                                lease.timer.action.kind = kind;
                                if oversized {
                                    request.limit =
                                        NonZeroU16::new(MAX_TIMER_CLAIM_BATCH + 1).unwrap();
                                }
                                assert_eq!(
                                    lease.validate_for_claim(&request),
                                    legacy(&lease, &request),
                                    "length={length} mask={mask} owner={other_owner} due={due} expires={expires} kind={kind:?} oversized={oversized}"
                                );
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    for (_profile, lease, request) in fixture::profiles() {
        assert_eq!(lease.validate_for_claim(&request), legacy(&lease, &request));
    }
    assert_eq!(cases, 18432);
    println!(
        "timer_legacy_cases={cases} profiles=32 competing_errors=true inclusive_boundaries=true"
    );
}
