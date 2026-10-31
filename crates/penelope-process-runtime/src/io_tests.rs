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

#[test]
fn execution_identity_preserves_legacy_encoding_and_generation_boundaries() {
    // Independent heap-based encoding freezes durable registry identities.
    for seed in 0_u32..10_000 {
        let bytes = Digest::of(&seed.to_be_bytes());
        let scope = TaskScope {
            tenant: TaskTenantId::from_digest(bytes),
            task: TaskId::from_digest(Digest::of(&bytes.0)),
            definition: TaskDefinitionFingerprint::from_digest(Digest::of(&seed.to_le_bytes())),
            input: TaskInputDigest::from_digest(Digest::of(&seed.wrapping_add(1).to_be_bytes())),
        };
        for value in [0, 1, 255, 256, u32::MAX] {
            let generation = TaskGeneration::new(value);
            let mut legacy_scope = b"penelope.linux-worker.scope.v1\0".to_vec();
            for digest in [
                scope.tenant.digest(),
                scope.task.digest(),
                scope.definition.digest(),
                scope.input.digest(),
            ] {
                legacy_scope.extend_from_slice(&digest.0);
            }
            let mut legacy = b"penelope.linux.single-process.v1\0".to_vec();
            legacy.extend_from_slice(&Digest::of(&legacy_scope).0);
            legacy.extend_from_slice(&value.to_be_bytes());
            assert_eq!(execution_id(scope, generation), Digest::of(&legacy));
        }
    }
}

struct SlowWriter {
    bytes: Vec<u8>,
}
impl Write for SlowWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Positive short writes must not reset or bypass the transfer budget.
        std::thread::sleep(Duration::from_millis(2));
        self.bytes.extend_from_slice(bytes.get(..1).unwrap());
        Ok(1)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn bounded_send_rejects_expiry_despite_continuous_positive_progress() {
    let mut output = SlowWriter { bytes: Vec::new() };
    assert_eq!(
        send(&mut output, b"abcdefgh", Duration::from_millis(3)),
        Err(TaskPortError::Runtime)
    );
    assert!(output.bytes.len() < 8);
}

#[test]
fn bounded_send_rejects_a_final_positive_write_finishing_after_deadline() {
    let mut output = SlowWriter { bytes: Vec::new() };
    assert_eq!(
        send(&mut output, b"a", Duration::from_millis(1)),
        Err(TaskPortError::Runtime)
    );
}

#[test]
fn startup_parts_share_deadline_and_expired_parts_do_not_write() {
    let mut output = writer(None, false);
    let deadline = Instant::now().checked_add(Duration::from_secs(1)).unwrap();
    send_until(&mut output, b"header", deadline).unwrap();
    // Represents the same startup deadline expiring before a later part.
    let expired = Instant::now();
    assert_eq!(
        send_until(&mut output, b"payload", expired),
        Err(TaskPortError::Runtime)
    );
    assert_eq!(output.bytes, b"header");
    send_until(&mut output, b"", expired).unwrap();
    assert_eq!(output.bytes, b"header");
}

#[test]
fn startup_parts_cannot_reset_budget_after_each_positive_write() {
    let mut output = SlowWriter { bytes: Vec::new() };
    assert_eq!(
        send_parts(
            &mut output,
            &[b"a", b"b", b"c", b"d"],
            Duration::from_millis(3)
        ),
        Err(TaskPortError::Runtime)
    );
    assert!(output.bytes.len() < 4);
}

struct TransientWriter {
    kind: ErrorKind,
    retries_per_chunk: usize,
    remaining: usize,
    attempts: usize,
    bytes: Vec<u8>,
}
impl Write for TransientWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.attempts = self.attempts.saturating_add(1);
        if self.remaining > 0 {
            self.remaining = self.remaining.saturating_sub(1);
            return Err(self.kind.into());
        }
        let chunk = bytes.get(..bytes.len().min(2)).unwrap();
        self.bytes.extend_from_slice(chunk);
        self.remaining = self.retries_per_chunk;
        Ok(chunk.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn retry_burst(kind: ErrorKind, retries_per_chunk: usize) {
    let mut output = TransientWriter {
        kind,
        retries_per_chunk,
        remaining: retries_per_chunk,
        attempts: 0,
        bytes: Vec::new(),
    };
    send_parts(&mut output, &[b"abcd", b"efgh"], Duration::from_secs(1)).unwrap();
    assert_eq!(output.bytes, b"abcdefgh");
    assert_eq!(
        output.attempts,
        retries_per_chunk.saturating_add(1).saturating_mul(4)
    );
}
#[test]
fn interrupted_bursts_preserve_bytes_across_short_writes_and_parts() {
    retry_burst(ErrorKind::Interrupted, 4);
}
#[test]
fn would_block_bursts_preserve_bytes_across_short_writes_and_parts() {
    retry_burst(ErrorKind::WouldBlock, 2);
}
struct AlwaysInterrupted {
    attempts: usize,
}
impl Write for AlwaysInterrupted {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        self.attempts = self.attempts.saturating_add(1);
        Err(ErrorKind::Interrupted.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn continuous_interruptions_do_not_reset_the_write_deadline() {
    let mut output = AlwaysInterrupted { attempts: 0 };
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(2))
        .unwrap();
    assert_eq!(
        send_until(&mut output, b"a", deadline),
        Err(TaskPortError::Runtime)
    );
    let attempts = output.attempts;
    println!("interrupted attempts before deadline: {attempts}");
    assert_eq!(
        send_until(&mut output, b"b", deadline),
        Err(TaskPortError::Runtime)
    );
    assert_eq!(output.attempts, attempts);
}
