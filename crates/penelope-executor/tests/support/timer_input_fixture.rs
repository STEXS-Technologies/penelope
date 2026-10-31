//! Stable timer identity fixtures; setup stays outside measured regions.
use penelope_domain::*;
use penelope_ports::{OutboxLeaseToken, TimerLease, TimerSchedule};
use std::num::NonZeroU64;
pub type Error = Box<dyn std::error::Error>;
pub fn lease(
    scope_len: usize,
    step_len: usize,
    action_len: usize,
    due: u64,
    attempt: u32,
) -> Result<TimerLease, Error> {
    let suffix = "x".repeat(scope_len.checked_sub(4).ok_or("short scope")?);
    let scope = ProcessScope::new(
        format!("tnt_{suffix}").parse()?,
        format!("prc_{suffix}").parse()?,
        format!("def_{suffix}").parse()?,
        format!("dfv_{suffix}").parse()?,
        ContentDigest([3; 32]),
    );
    let action = ProcessAction::new(
        scope,
        format!(
            "act_{}",
            "x".repeat(action_len.checked_sub(4).ok_or("short action")?)
        )
        .parse()?,
        format!(
            "stp_{}",
            "x".repeat(step_len.checked_sub(4).ok_or("short step")?)
        )
        .parse()?,
        attempt,
        ProcessActionKind::Timer,
        ContentDigest([4; 32]),
    );
    Ok(TimerLease {
        timer: TimerSchedule {
            action,
            due_at: LogicalTime(due),
        },
        owner: "pri_timer".parse()?,
        token: OutboxLeaseToken::new(NonZeroU64::MIN),
        lease_expires_at: LogicalTime(u64::MAX),
    })
}
pub fn profiles() -> Result<Vec<(String, TimerLease)>, Error> {
    let mut profiles = Vec::new();
    for scope in [5, 22, 23, 128] {
        for step in [5, 128] {
            for action in [5, 128] {
                profiles.push((
                    format!("scope-{scope}-step-{step}-action-{action}"),
                    lease(scope, step, action, 100, u32::MAX)?,
                ));
            }
        }
    }
    Ok(profiles)
}
