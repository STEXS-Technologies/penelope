//! Bounded IPC write failures and scalar frame decoding.
#![allow(clippy::unwrap_used)]
use super::*;
use std::io::{self, ErrorKind};

struct Writer {
    first: Option<ErrorKind>,
    bytes: Vec<u8>,
    zero: bool,
}
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(kind) = self.first.take() {
            return Err(kind.into());
        }
        if self.zero {
            return Ok(0);
        }
        let chunk = bytes.get(..bytes.len().min(2)).unwrap();
        self.bytes.extend_from_slice(chunk);
        Ok(chunk.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn writer(first: Option<ErrorKind>, zero: bool) -> Writer {
    Writer {
        first,
        bytes: Vec::new(),
        zero,
    }
}
#[test]
fn bounded_send_retries_transient_errors_without_duplicate_or_missing_bytes() {
    for kind in [ErrorKind::Interrupted, ErrorKind::WouldBlock] {
        let mut output = writer(Some(kind), false);
        send(&mut output, b"abcdefg", Duration::from_secs(1)).unwrap();
        assert_eq!(output.bytes, b"abcdefg");
    }
}
#[test]
fn bounded_send_rejects_zero_permanent_error_and_expired_retry() {
    let mut zero = writer(None, true);
    assert_eq!(
        send(&mut zero, b"a", Duration::from_secs(1)),
        Err(TaskPortError::Runtime)
    );
    let mut broken = writer(Some(ErrorKind::BrokenPipe), false);
    assert_eq!(
        send(&mut broken, b"a", Duration::from_secs(1)),
        Err(TaskPortError::Runtime)
    );
    let mut blocked = writer(Some(ErrorKind::WouldBlock), false);
    assert_eq!(
        send(&mut blocked, b"a", Duration::ZERO),
        Err(TaskPortError::Runtime)
    );
    assert!(blocked.bytes.is_empty());
    let mut overflow = writer(None, false);
    assert_eq!(
        send(&mut overflow, b"a", Duration::MAX),
        Err(TaskPortError::Runtime)
    );
    assert!(overflow.bytes.is_empty());
}
#[test]
fn frame_number_rejects_truncation_and_offset_overflow() {
    assert_eq!(number(&[7, 0, 0, 0, 42], 1), Ok(42));
    for offset in [2, usize::MAX] {
        assert_eq!(
            number(&[7, 0, 0, 0, 42], offset),
            Err(TaskPortError::Corrupt)
        );
    }
}

#[test]
fn exit_between_empty_read_and_status_check_cannot_discard_queued_completion() {
    // At WouldBlock or after a partial frame, even an exited child does not
    // prove the pipe is drained. Its final write may follow the earlier read.
    let checked = std::cell::Cell::new(false);
    let progress = progress_after_read(false, || {
        checked.set(true);
        Ok(Some(ExitStatus::from_raw(0)))
    })
    .unwrap();
    assert_eq!(progress, RuntimeProgress::Pending);
    assert!(!checked.get());
}

#[test]
fn eof_without_a_complete_frame_preserves_terminal_fault_classification() {
    for (status, expected) in [
        (0, TaskFault::ApplicationFailure),
        (libc::SIGSYS, TaskFault::ProtectionViolation),
        (libc::SIGXCPU, TaskFault::CpuBudgetExceeded),
        (libc::SIGKILL, TaskFault::Crash),
    ] {
        assert_eq!(
            progress_after_read(true, || Ok(Some(ExitStatus::from_raw(status)))).unwrap(),
            RuntimeProgress::Fault(expected)
        );
    }
    assert_eq!(
        progress_after_read(true, || Ok(None)).unwrap(),
        RuntimeProgress::Pending
    );
    assert_eq!(
        progress_after_read(true, || Err(io::Error::from(ErrorKind::Other))),
        Err(TaskPortError::Runtime)
    );
}
