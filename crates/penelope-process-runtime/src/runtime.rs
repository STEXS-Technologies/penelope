use crate::{
    image::ProgramImage,
    registry::Registry,
    sandbox,
    worker::{OBS_HEADER, OBS_MAGIC, START_MAGIC, WorkerMetadata, scope_digest},
};
use penelope_core::task::{runtime::*, *};
use rustix::{
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    process::{
        Pid, PidfdFlags, Signal, WaitId, WaitIdOptions, pidfd_open, pidfd_send_signal, waitid,
    },
};
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsFd, AsRawFd},
        unix::process::ExitStatusExt,
    },
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

/// Explicit supported profile: static Linux x86-64 ELF, one process and thread,
/// stdin/stdout IPC, no filesystem/network/process-creation syscalls. Registry
/// ownership and a trusted host kernel are deployment preconditions.
pub struct ProcessRuntimeConfig {
    pub registry: PathBuf,
    pub worker: PathBuf,
    pub max_worker_bytes: u64,
    pub max_frame_bytes: u32,
    pub max_live: u16,
    /// Trusted task tick frequency. CPU allowances must be whole OS seconds.
    pub ticks_per_second: u64,
    pub address_space_bytes: u64,
    /// One deadline for the complete startup-frame write, or one checkpoint ACK.
    /// Does not bound image loading, process creation or parent scheduling delays.
    pub startup_timeout: Duration,
    pub fence_timeout: Duration,
}

struct Active {
    scope: TaskScope,
    generation: TaskGeneration,
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    buffer: Vec<u8>,
    filled: usize,
    checkpoint: Option<BlobRef>,
    definition: TaskDefinition,
}
/// Protected process runtime joined to an injected durable generation authority.
/// Dropping it does not acknowledge fences. Reconstruct and fence using the same
/// private registry after interruption; do not delete registry evidence.
pub struct LinuxProcessRuntime<A> {
    authority: A,
    registry: Registry,
    image: ProgramImage,
    config: ProcessRuntimeConfig,
    active: Vec<Active>,
}
fn execution_id(scope: TaskScope, generation: TaskGeneration) -> Digest {
    const PREFIX: &[u8] = b"penelope.linux.single-process.v1\0";
    let mut bytes = [0; PREFIX.len().saturating_add(36)];
    let (prefix, fields) = bytes.split_at_mut(PREFIX.len());
    prefix.copy_from_slice(PREFIX);
    let (scope_bytes, generation_bytes) = fields.split_at_mut(32);
    scope_bytes.copy_from_slice(&scope_digest(scope).0);
    generation_bytes.copy_from_slice(&generation.get().to_be_bytes());
    Digest::of(&bytes)
}
fn number(bytes: &[u8], offset: usize) -> Result<u32, TaskPortError> {
    Ok(u32::from_be_bytes(
        bytes
            .get(offset..offset.checked_add(4).ok_or(TaskPortError::Corrupt)?)
            .ok_or(TaskPortError::Corrupt)?
            .try_into()
            .map_err(|_size| TaskPortError::Corrupt)?,
    ))
}
fn nonblocking(fd: &impl std::os::fd::AsFd) -> Result<(), TaskPortError> {
    let flags = fcntl_getfl(fd).map_err(|_flags| TaskPortError::Runtime)?;
    fcntl_setfl(fd, flags | OFlags::NONBLOCK).map_err(|_flags| TaskPortError::Runtime)
}
fn send(writer: &mut impl Write, bytes: &[u8], timeout: Duration) -> Result<(), TaskPortError> {
    send_parts(writer, &[bytes], timeout)
}
fn send_parts(
    writer: &mut impl Write,
    parts: &[&[u8]],
    timeout: Duration,
) -> Result<(), TaskPortError> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(TaskPortError::Runtime)?;
    for bytes in parts {
        send_until(writer, bytes, deadline)?;
    }
    Ok(())
}
fn send_until(
    writer: &mut impl Write,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), TaskPortError> {
    let mut offset = 0;
    while offset < bytes.len() {
        if Instant::now() >= deadline {
            return Err(TaskPortError::Runtime);
        }
        match writer.write(bytes.get(offset..).ok_or(TaskPortError::Corrupt)?) {
            Ok(0) => return Err(TaskPortError::Runtime),
            Ok(count) => offset = offset.checked_add(count).ok_or(TaskPortError::Runtime)?,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                if Instant::now() >= deadline {
                    return Err(TaskPortError::Runtime);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(_write) => return Err(TaskPortError::Runtime),
        }
    }
    // A final positive write may also finish after the deadline. Bytes already
    // written cannot be recalled; the caller treats failure as ambiguous.
    if !bytes.is_empty() && Instant::now() >= deadline {
        return Err(TaskPortError::Runtime);
    }
    Ok(())
}
// A worker may write and exit after a nonblocking read reports WouldBlock.
// Only EOF proves that no observation bytes remain to be consumed. Reaping the
// child before that point can turn a queued valid completion into a false fault.
fn progress_after_read(
    reached_eof: bool,
    exit_status: impl FnOnce() -> std::io::Result<Option<ExitStatus>>,
) -> Result<RuntimeProgress, TaskPortError> {
    if !reached_eof {
        return Ok(RuntimeProgress::Pending);
    }
    Ok(exit_status()
        .map_err(|_wait| TaskPortError::Runtime)?
        .map_or(RuntimeProgress::Pending, |status| {
            RuntimeProgress::Fault(match status.signal() {
                Some(libc::SIGSYS) => TaskFault::ProtectionViolation,
                Some(libc::SIGXCPU) => TaskFault::CpuBudgetExceeded,
                Some(_) => TaskFault::Crash,
                None => TaskFault::ApplicationFailure,
            })
        }))
}
impl<A: TaskGenerationAuthority> LinuxProcessRuntime<A> {
    /// Pins a sealed static ELF and validates Linux feature/configuration bounds.
    /// # Errors
    /// Rejects dynamic/wrong-architecture images, unavailable pidfds, bad bounds
    /// or an unusable private registry. No execution is authorized here.
    pub fn new(authority: A, config: ProcessRuntimeConfig) -> Result<Self, TaskPortError> {
        if config.max_live == 0
            || config.max_live > 256
            || config.max_frame_bytes == 0
            || config.max_frame_bytes > 16 * 1024 * 1024
            || config.ticks_per_second == 0
            || config.address_space_bytes < 8 * 1024 * 1024
            || config.startup_timeout.is_zero()
            || config.startup_timeout > Duration::from_secs(30)
            || config.fence_timeout.is_zero()
            || config.fence_timeout > Duration::from_secs(30)
        {
            return Err(TaskPortError::Exhausted);
        }
        pidfd_open(rustix::process::getpid(), PidfdFlags::empty())
            .map_err(|_pidfd| TaskPortError::Runtime)?;
        let registry = Registry::open(&config.registry, config.fence_timeout)?;
        let image = ProgramImage::load(&config.worker, config.max_worker_bytes)?;
        let active = Vec::with_capacity(usize::from(config.max_live));
        Ok(Self {
            authority,
            registry,
            image,
            config,
            active,
        })
    }
    /// Returns the sealed bytes' code identity for a pinned task definition.
    #[must_use]
    pub const fn code_identity(&self) -> Digest {
        self.image.digest
    }
    fn index(&self, scope: TaskScope, generation: TaskGeneration) -> Result<usize, TaskPortError> {
        self.active
            .iter()
            .position(|active| active.scope == scope && active.generation == generation)
            .ok_or(TaskPortError::Conflict)
    }
}
impl<A: TaskGenerationAuthority> TaskRuntime for LinuxProcessRuntime<A> {
    fn start(&mut self, request: TaskStart<'_>) -> Result<(), TaskPortError> {
        if request.definition.code != self.image.digest
            || request
                .definition
                .budget
                .cpu_ticks
                .checked_rem(self.config.ticks_per_second)
                != Some(0)
            || request.definition.budget.cpu_ticks < self.config.ticks_per_second
            || request.definition.budget.max_input_bytes > self.config.max_frame_bytes
            || request.definition.budget.max_output_bytes > self.config.max_frame_bytes
            || request.definition.recovery.max_checkpoint_bytes > self.config.max_frame_bytes
            || (self.active.len() >= usize::from(self.config.max_live)
                && self.index(request.scope, request.generation).is_err())
        {
            return Err(TaskPortError::Exhausted);
        }
        let _guard = self.registry.lock()?;
        let execution = execution_id(request.scope, request.generation);
        match self.authority.reserve_start(request, execution)? {
            StartReservation::AlreadyReserved => {
                return if self.index(request.scope, request.generation).is_ok() {
                    Ok(())
                } else {
                    Err(TaskPortError::Conflict)
                };
            }
            StartReservation::Reserved => {}
        }
        let manifest = self
            .registry
            .prepare(request.scope, request.generation, execution)?;
        let mut command = Command::new(format!("/proc/self/fd/{}", self.image.file.as_raw_fd()));
        command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = sandbox::spawn(
            &mut command,
            &self.image.file,
            manifest,
            request
                .definition
                .budget
                .cpu_ticks
                .checked_div(self.config.ticks_per_second)
                .ok_or(TaskPortError::Runtime)?,
            self.config.address_space_bytes,
        )?;
        let stdin = child.stdin.take().ok_or(TaskPortError::Runtime)?;
        let stdout = child.stdout.take().ok_or(TaskPortError::Runtime)?;
        let size = usize::try_from(
            request
                .definition
                .budget
                .max_output_bytes
                .max(request.definition.recovery.max_checkpoint_bytes),
        )
        .map_err(|_size| TaskPortError::Exhausted)?
        // Read one byte beyond the largest valid frame so coalesced trailing
        // data cannot hide behind an exactly full buffer.
        .checked_add(OBS_HEADER)
        .and_then(|size| size.checked_add(1))
        .ok_or(TaskPortError::Exhausted)?;
        self.active.push(Active {
            scope: request.scope,
            generation: request.generation,
            child,
            stdin,
            stdout,
            buffer: vec![0; size],
            filled: 0,
            checkpoint: None,
            definition: request.definition,
        });
        let index = self.index(request.scope, request.generation)?;
        let active = self.active.get_mut(index).ok_or(TaskPortError::Runtime)?;
        nonblocking(&active.stdin)?;
        nonblocking(&active.stdout)?;
        let metadata = serde_json::to_vec(&WorkerMetadata {
            scope: request.scope,
            generation: request.generation,
            definition: request.definition,
            execution_deadline: request.execution_deadline,
            input_schema: request.input_schema,
        })
        .map_err(|_codec| TaskPortError::Corrupt)?;
        if metadata.len() > 8192 {
            return Err(TaskPortError::Exhausted);
        }
        let mut header = Vec::with_capacity(60);
        header.extend_from_slice(START_MAGIC);
        header.extend_from_slice(&scope_digest(request.scope).0);
        header.extend_from_slice(&request.generation.get().to_be_bytes());
        for value in [
            u32::try_from(metadata.len()).map_err(|_length| TaskPortError::Exhausted)?,
            u32::try_from(request.input.len()).map_err(|_length| TaskPortError::Exhausted)?,
            u32::try_from(request.checkpoint.map_or(0, <[u8]>::len))
                .map_err(|_length| TaskPortError::Exhausted)?,
            u32::from(request.checkpoint.is_some()),
        ] {
            header.extend_from_slice(&value.to_be_bytes());
        }
        send_parts(
            &mut active.stdin,
            &[
                header.as_slice(),
                metadata.as_slice(),
                request.input,
                request.checkpoint.unwrap_or(&[]),
            ],
            self.config.startup_timeout,
        )
    }
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError> {
        let index = self.index(scope, generation)?;
        let active = self.active.get_mut(index).ok_or(TaskPortError::Conflict)?;
        if active.checkpoint.is_some() {
            return Ok(TaskObservation {
                scope,
                generation,
                progress: RuntimeProgress::Pending,
            });
        }
        let remaining = active
            .buffer
            .get_mut(active.filled..)
            .ok_or(TaskPortError::Corrupt)?;
        let mut reached_eof = false;
        if !remaining.is_empty() {
            match active.stdout.read(remaining) {
                Ok(0) => reached_eof = true,
                Ok(count) => {
                    active.filled = active
                        .filled
                        .checked_add(count)
                        .ok_or(TaskPortError::Corrupt)?
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_read) => return Err(TaskPortError::Runtime),
            }
        }
        if active.filled >= OBS_HEADER {
            let header = active
                .buffer
                .get(..OBS_HEADER)
                .ok_or(TaskPortError::Corrupt)?;
            if header.get(..8) != Some(OBS_MAGIC)
                || header.get(8..40) != Some(scope_digest(scope).0.as_slice())
                || number(header, 40)? != generation.get()
            {
                return Err(TaskPortError::Corrupt);
            }
            let checkpoint = match header.get(44) {
                Some(1) => false,
                Some(2) => true,
                _ => return Err(TaskPortError::Corrupt),
            };
            let length = number(header, 45)?;
            let schema = number(header, 49)?;
            let limit = if checkpoint {
                active.definition.recovery.max_checkpoint_bytes
            } else {
                active.definition.budget.max_output_bytes
            };
            if length > limit || schema == 0 {
                return Err(TaskPortError::Exhausted);
            }
            let length = usize::try_from(length).map_err(|_length| TaskPortError::Exhausted)?;
            let end = OBS_HEADER
                .checked_add(length)
                .ok_or(TaskPortError::Exhausted)?;
            if active.filled >= end {
                if active.filled != end {
                    return Err(TaskPortError::Corrupt);
                }
                let bytes = active
                    .buffer
                    .get(OBS_HEADER..end)
                    .ok_or(TaskPortError::Corrupt)?;
                buffer
                    .get_mut(..length)
                    .ok_or(TaskPortError::Exhausted)?
                    .copy_from_slice(bytes);
                if checkpoint {
                    active.checkpoint = Some(BlobRef {
                        digest: Digest::of(bytes),
                        bytes: u32::try_from(length).map_err(|_length| TaskPortError::Exhausted)?,
                        schema,
                    });
                }
                active.filled = 0;
                return Ok(TaskObservation {
                    scope,
                    generation,
                    progress: if checkpoint {
                        RuntimeProgress::Checkpoint {
                            bytes: u32::try_from(length)
                                .map_err(|_length| TaskPortError::Exhausted)?,
                            schema,
                        }
                    } else {
                        RuntimeProgress::Complete {
                            bytes: u32::try_from(length)
                                .map_err(|_length| TaskPortError::Exhausted)?,
                            schema,
                        }
                    },
                });
            }
        }
        let progress = progress_after_read(reached_eof, || active.child.try_wait())?;
        Ok(TaskObservation {
            scope,
            generation,
            progress,
        })
    }
    fn acknowledge_checkpoint(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        checkpoint: BlobRef,
    ) -> Result<(), TaskPortError> {
        let index = self.index(scope, generation)?;
        let active = self.active.get_mut(index).ok_or(TaskPortError::Conflict)?;
        if active.checkpoint != Some(checkpoint) {
            return Err(TaskPortError::Conflict);
        }
        let mut bytes = [0; 40];
        let (magic, digest) = bytes.split_at_mut(8);
        magic.copy_from_slice(b"PNLACK01");
        digest.copy_from_slice(&checkpoint.digest.0);
        send(&mut active.stdin, &bytes, self.config.startup_timeout)?;
        active.checkpoint = None;
        Ok(())
    }
    fn fence(&mut self, scope: TaskScope, generation: TaskGeneration) -> Result<(), TaskPortError> {
        let _guard = self.registry.lock()?;
        let execution = self.authority.begin_fence(scope, generation)?;
        let mut stopped_pid = None;
        if let Some(execution) = execution {
            if execution != execution_id(scope, generation) {
                return Err(TaskPortError::Corrupt);
            }
            if let Some(saved) = self.registry.load(scope, generation, execution)? {
                if saved.boot == self.registry.boot {
                    if let Some(identity) = saved.identity {
                        stop_identity(identity, self.config.fence_timeout)?;
                        stopped_pid = Some(identity.pid);
                    }
                }
            }
        }
        if let Ok(index) = self.index(scope, generation) {
            let active = self.active.get_mut(index).ok_or(TaskPortError::Runtime)?;
            match active.child.try_wait() {
                Ok(Some(_status)) => {}
                // Another runtime in this process may already have reaped the
                // same worker through its pidfd. Accept that only after the
                // registry identity for this exact child was proved stopped.
                Err(error)
                    if error.raw_os_error() == Some(libc::ECHILD)
                        && stopped_pid == Some(active.child.id()) => {}
                Ok(None) | Err(_) => return Err(TaskPortError::Runtime),
            }
            self.active.swap_remove(index);
        }
        self.authority.confirm_stopped(scope, generation, execution)
    }
}
fn stop_identity(
    identity: sandbox::ProcessIdentity,
    timeout: Duration,
) -> Result<(), TaskPortError> {
    let pid = Pid::from_raw(i32::try_from(identity.pid).map_err(|_pid| TaskPortError::Corrupt)?)
        .ok_or(TaskPortError::Corrupt)?;
    let fd = match pidfd_open(pid, PidfdFlags::empty()) {
        Ok(fd) => fd,
        Err(error) if error == rustix::io::Errno::SRCH => return Ok(()),
        Err(_open) => return Err(TaskPortError::Runtime),
    };
    let mut bytes = Vec::new();
    match File::open(format!("/proc/{}/stat", identity.pid)) {
        Ok(file) => {
            file.take(4096)
                .read_to_end(&mut bytes)
                .map_err(|_read| TaskPortError::Runtime)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_open) => return Err(TaskPortError::Runtime),
    }
    let observed = sandbox::parse_identity(identity.pid, &bytes)
        .map_err(|_identity| TaskPortError::Corrupt)?;
    if observed != identity {
        return Ok(());
    }
    match pidfd_send_signal(&fd, Signal::KILL) {
        Ok(()) => {}
        Err(error) if error == rustix::io::Errno::SRCH => {}
        Err(_kill) => return Err(TaskPortError::Runtime),
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(TaskPortError::Runtime)?;
    loop {
        let mut descriptors = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(
            &mut descriptors,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )
        .map_err(|_poll| TaskPortError::Runtime)?;
        if descriptors.first().is_some_and(|descriptor| {
            descriptor
                .revents()
                .intersects(rustix::event::PollFlags::IN | rustix::event::PollFlags::HUP)
        }) {
            // A dropped Child handle does not reap its process. Reconstruction
            // in the same parent must release that zombie through the pinned
            // pidfd, never a PID that could have been reused. Non-child workers
            // are reaped by their own parent; readiness already proved exit.
            return match waitid(
                WaitId::PidFd(fd.as_fd()),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
            ) {
                Ok(Some(_)) | Err(rustix::io::Errno::CHILD) => Ok(()),
                Ok(None) | Err(_) => Err(TaskPortError::Runtime),
            };
        }
        if Instant::now() >= deadline {
            return Err(TaskPortError::Runtime);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
#[path = "fence_tests.rs"]
mod fence_tests;

#[cfg(test)]
#[path = "io_tests.rs"]
mod io_tests;
