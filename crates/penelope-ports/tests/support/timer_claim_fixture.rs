//! Timer claim fixtures; construction stays outside measured regions.
#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
use penelope_domain::*;
use penelope_ports::*;
use std::num::{NonZeroU16, NonZeroU64};
pub fn fixture(length: usize) -> (TimerLease, TimerClaimRequest) {
    let suffix = "x".repeat(length - 4);
    let scope = ProcessScope::new(
        format!("tnt_{suffix}").parse().unwrap(),
        format!("prc_{suffix}").parse().unwrap(),
        format!("def_{suffix}").parse().unwrap(),
        format!("dfv_{suffix}").parse().unwrap(),
        ContentDigest([3; 32]),
    );
    let owner = "pri_timer".parse().unwrap();
    let request =
        TimerClaimRequest::new(scope.clone(), owner, LogicalTime(100), NonZeroU16::MIN).unwrap();
    let lease = TimerLease {
        timer: TimerSchedule {
            action: ProcessAction::new(
                scope,
                "act_timer".parse().unwrap(),
                "stp_timer".parse().unwrap(),
                0,
                ProcessActionKind::Timer,
                ContentDigest([4; 32]),
            ),
            due_at: LogicalTime(100),
        },
        owner: request.owner.clone(),
        token: OutboxLeaseToken::new(NonZeroU64::MIN),
        lease_expires_at: LogicalTime(100),
    };
    (lease, request)
}
pub fn profiles() -> Vec<(String, TimerLease, TimerClaimRequest)> {
    let mut profiles = Vec::new();
    for length in [5, 22, 23, 128] {
        for case in [
            "valid",
            "first-scope",
            "last-scope",
            "owner",
            "not-due",
            "expired",
            "bad-kind",
            "oversized-limit",
        ] {
            let (mut lease, mut request) = fixture(length);
            match case {
                "first-scope" => request.scope.tenant_id = "tnt_other".parse().unwrap(),
                "last-scope" => request.scope.definition_digest = ContentDigest([8; 32]),
                "owner" => request.owner = "pri_other".parse().unwrap(),
                "not-due" => lease.timer.due_at = LogicalTime(101),
                "expired" => lease.lease_expires_at = LogicalTime(99),
                "bad-kind" => lease.timer.action.kind = ProcessActionKind::ExternalEffect,
                "oversized-limit" => {
                    request.limit = NonZeroU16::new(MAX_TIMER_CLAIM_BATCH + 1).unwrap()
                }
                _ => {}
            }
            profiles.push((format!("{length}-{case}"), lease, request));
        }
    }
    profiles
}
