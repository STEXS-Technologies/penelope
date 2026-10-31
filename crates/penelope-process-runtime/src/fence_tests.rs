//! PID-reuse qualification for the actual pidfd termination boundary. Every PID
//! belongs to a child created by this test; no unrelated host process is signaled.
#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;

#[test]
fn stale_start_time_cannot_signal_a_reused_pid() {
    let mut child = Command::new("sleep").arg("60").spawn().unwrap();
    let bytes = std::fs::read(format!("/proc/{}/stat", child.id())).unwrap();
    let current = sandbox::parse_identity(child.id(), &bytes).unwrap();
    let stale = sandbox::ProcessIdentity {
        start_ticks: current.start_ticks.saturating_sub(1),
        ..current
    };
    // A pidfd pins the process we opened, but the kernel start time still has
    // to match the saved generation before any signal may be sent.
    stop_identity(stale, Duration::from_secs(1)).unwrap();
    assert!(child.try_wait().unwrap().is_none());
    stop_identity(current, Duration::from_secs(1)).unwrap();
    assert_eq!(
        child.try_wait().unwrap_err().raw_os_error(),
        Some(libc::ECHILD)
    );
    assert!(!std::path::Path::new(&format!("/proc/{}", current.pid)).exists());
    // The old identity is already stopped, even once the child has been reaped.
    stop_identity(current, Duration::from_secs(1)).unwrap();
}

#[test]
fn impossible_saved_pids_are_corruption_without_signaling() {
    for pid in [0, u32::MAX] {
        assert_eq!(
            stop_identity(
                sandbox::ProcessIdentity {
                    pid,
                    start_ticks: 1
                },
                Duration::from_secs(1)
            ),
            Err(TaskPortError::Corrupt)
        );
    }
}

#[test]
fn kernel_identity_parser_preserves_command_delimiters_and_rejects_bad_start_times() {
    let fields = std::iter::repeat_n("0", 19).collect::<Vec<_>>().join(" ");
    let stat = format!("123 (worker with ) embedded delimiters) {fields} 456 0 0");
    assert_eq!(
        sandbox::parse_identity(123, stat.as_bytes()).unwrap(),
        sandbox::ProcessIdentity {
            pid: 123,
            start_ticks: 456
        }
    );
    for malformed in [
        b"".as_slice(),
        b"123 no delimiter",
        b"123 (worker) 0",
        b"123 (worker) \xff",
    ] {
        assert!(sandbox::parse_identity(123, malformed).is_err());
    }
    for start in ["invalid", "-1", "18446744073709551616"] {
        let malformed = format!("123 (worker) {fields} {start}");
        assert!(sandbox::parse_identity(123, malformed.as_bytes()).is_err());
    }
}

#[test]
fn unrepresentable_fence_deadline_preserves_error_after_signaling_owned_child() {
    let mut child = Command::new("sleep").arg("60").spawn().unwrap();
    let bytes = std::fs::read(format!("/proc/{}/stat", child.id())).unwrap();
    let identity = sandbox::parse_identity(child.id(), &bytes).unwrap();
    assert_eq!(
        stop_identity(identity, Duration::MAX),
        Err(TaskPortError::Runtime)
    );
    assert!(!child.wait().unwrap().success());
    // A failed stop result cannot be treated as confirmed authority. Retrying
    // from durable evidence can confirm the process is now gone.
    stop_identity(identity, Duration::from_secs(1)).unwrap();
}
