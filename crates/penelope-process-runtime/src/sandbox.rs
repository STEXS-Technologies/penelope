//! The only post-fork unsafe boundary. Prepare all allocations in the parent;
//! child setup uses stack parsing and async-signal-safe Linux syscalls only.
#![allow(unsafe_code)]
use penelope_core::task::runtime::TaskPortError;
use rustix::{
    fs::{Mode, OFlags},
    process::{
        Resource, Rlimit, Signal, getpid, getppid, set_parent_process_death_signal, setrlimit,
    },
};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io,
    os::unix::{
        io::{AsRawFd, RawFd},
        process::CommandExt,
    },
    process::{Child, Command},
};

pub(crate) const IDENTITY_OFFSET: u64 = 160;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: u32,
    pub(crate) start_ticks: u64,
}

fn filter(executable: RawFd) -> Result<BpfProgram, TaskPortError> {
    let mut rules = BTreeMap::new();
    for syscall in [
        libc::SYS_brk,
        libc::SYS_mmap,
        libc::SYS_mprotect,
        libc::SYS_munmap,
        libc::SYS_mremap,
        libc::SYS_madvise,
        libc::SYS_close,
        libc::SYS_fstat,
        libc::SYS_poll,
        libc::SYS_ppoll,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        libc::SYS_futex,
        libc::SYS_arch_prctl,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_rseq,
        libc::SYS_getpid,
        libc::SYS_getppid,
        libc::SYS_gettid,
        libc::SYS_getuid,
        libc::SYS_geteuid,
        libc::SYS_getgid,
        libc::SYS_getegid,
        libc::SYS_clock_gettime,
        libc::SYS_clock_getres,
        libc::SYS_nanosleep,
        libc::SYS_clock_nanosleep,
        libc::SYS_getrandom,
        libc::SYS_sched_yield,
        libc::SYS_getrlimit,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ] {
        rules.insert(syscall, Vec::new());
    }
    for (syscall, descriptors) in [
        (libc::SYS_read, vec![0]),
        (libc::SYS_readv, vec![0]),
        (libc::SYS_write, vec![1, 2]),
        (libc::SYS_writev, vec![1, 2]),
    ] {
        let mut alternatives = Vec::new();
        for descriptor in descriptors {
            alternatives.push(
                SeccompRule::new(vec![
                    SeccompCondition::new(0, SeccompCmpArgLen::Qword, SeccompCmpOp::Eq, descriptor)
                        .map_err(|_condition| TaskPortError::Runtime)?,
                ])
                .map_err(|_rule| TaskPortError::Runtime)?,
            );
        }
        rules.insert(syscall, alternatives);
    }
    // Only the sealed bootstrap descriptor may be executed. It is CLOEXEC, so
    // the worker cannot execute it again; descriptor-creating/duplicating calls
    // are absent from the allowlist and cannot rebind this number after exec.
    rules.insert(
        libc::SYS_execveat,
        vec![
            SeccompRule::new(vec![
                SeccompCondition::new(
                    0,
                    SeccompCmpArgLen::Qword,
                    SeccompCmpOp::Eq,
                    u64::try_from(executable).map_err(|_fd| TaskPortError::Runtime)?,
                )
                .map_err(|_condition| TaskPortError::Runtime)?,
                SeccompCondition::new(
                    4,
                    SeccompCmpArgLen::Qword,
                    SeccompCmpOp::Eq,
                    u64::try_from(libc::AT_EMPTY_PATH).map_err(|_flags| TaskPortError::Runtime)?,
                )
                .map_err(|_condition| TaskPortError::Runtime)?,
            ])
            .map_err(|_rule| TaskPortError::Runtime)?,
        ],
    );
    // Rust's static runtime reads its stack limit; no caller may change limits.
    rules.insert(
        libc::SYS_prlimit64,
        vec![
            SeccompRule::new(vec![
                SeccompCondition::new(0, SeccompCmpArgLen::Qword, SeccompCmpOp::Eq, 0)
                    .map_err(|_condition| TaskPortError::Runtime)?,
                SeccompCondition::new(2, SeccompCmpArgLen::Qword, SeccompCmpOp::Eq, 0)
                    .map_err(|_condition| TaskPortError::Runtime)?,
            ])
            .map_err(|_rule| TaskPortError::Runtime)?,
        ],
    );
    SeccompFilter::new(
        rules,
        SeccompAction::KillProcess,
        SeccompAction::Allow,
        TargetArch::x86_64,
    )
    .map_err(|_filter| TaskPortError::Runtime)?
    .try_into()
    .map_err(|_compile| TaskPortError::Runtime)
}

fn syscall_error(_error: rustix::io::Errno) -> io::Error {
    io::Error::from_raw_os_error(libc::EPERM)
}
fn self_identity() -> io::Result<ProcessIdentity> {
    let file = rustix::fs::open(
        c"/proc/self/stat",
        OFlags::RDONLY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(syscall_error)?;
    let mut buffer = [0; 4096];
    let count = rustix::io::read(&file, &mut buffer).map_err(syscall_error)?;
    parse_identity(
        u32::try_from(getpid().as_raw_nonzero().get())
            .map_err(|_pid| io::Error::from_raw_os_error(libc::EINVAL))?,
        buffer
            .get(..count)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EIO))?,
    )
}
pub(crate) fn parse_identity(pid: u32, stat: &[u8]) -> io::Result<ProcessIdentity> {
    let error = || io::Error::from_raw_os_error(libc::EINVAL);
    let end = stat
        .iter()
        .rposition(|byte| *byte == b')')
        .ok_or_else(error)?;
    let fields = std::str::from_utf8(stat.get(end.saturating_add(1)..).ok_or_else(error)?)
        .map_err(|_utf8| error())?;
    let start_ticks = fields
        .split_ascii_whitespace()
        .nth(19)
        .ok_or_else(error)?
        .parse()
        .map_err(|_number| error())?;
    Ok(ProcessIdentity { pid, start_ticks })
}

// Kept separate so the irreversible child-only setup can be qualified in an
// isolated process before exec discards LLVM counters. Do not add allocations,
// locks, environment access or callbacks: this also runs after fork.
fn prepare_child(
    parent: rustix::process::Pid,
    manifest: &File,
    cpu_seconds: u64,
    address_space: u64,
) -> io::Result<()> {
    set_parent_process_death_signal(Some(Signal::KILL)).map_err(syscall_error)?;
    if getppid() != Some(parent) {
        return Err(io::Error::from_raw_os_error(libc::ECHILD));
    }
    let identity = self_identity()?;
    let mut bytes = [0; 12];
    bytes
        .get_mut(..4)
        .ok_or_else(|| io::Error::from_raw_os_error(libc::EIO))?
        .copy_from_slice(&identity.pid.to_be_bytes());
    bytes
        .get_mut(4..)
        .ok_or_else(|| io::Error::from_raw_os_error(libc::EIO))?
        .copy_from_slice(&identity.start_ticks.to_be_bytes());
    if rustix::io::pwrite(manifest, &bytes, IDENTITY_OFFSET).map_err(syscall_error)? != bytes.len()
    {
        return Err(io::Error::from_raw_os_error(libc::EIO));
    }
    rustix::fs::fsync(manifest).map_err(syscall_error)?;
    for (resource, limit) in [
        (Resource::Cpu, cpu_seconds),
        (Resource::As, address_space),
        (Resource::Core, 0),
    ] {
        setrlimit(
            resource,
            Rlimit {
                current: Some(limit),
                maximum: Some(limit),
            },
        )
        .map_err(syscall_error)?;
    }
    // SAFETY: close_range with CLOEXEC marks inherited descriptors for
    // closing on successful exec without closing Rust's exec-error pipe.
    if unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3_u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn spawn(
    command: &mut Command,
    image: &File,
    manifest: File,
    cpu_seconds: u64,
    address_space: u64,
) -> Result<Child, TaskPortError> {
    if cpu_seconds == 0 || address_space < 8 * 1024 * 1024 {
        return Err(TaskPortError::Exhausted);
    }
    // Keep the executable above stdio even if the host started with closed
    // standard descriptors. Prepare both the duplicate and BPF before fork.
    let executable =
        rustix::io::fcntl_dupfd_cloexec(image, 3).map_err(|_duplicate| TaskPortError::Runtime)?;
    let program = filter(executable.as_raw_fd())?;
    let parent = getpid();
    command.process_group(0);
    // SAFETY: the closure performs no allocation, locking, formatting, environment
    // access or application callbacks. All captured descriptors/program bytes are
    // prepared before fork. Linux syscall wrappers only touch owned descriptors
    // and stack storage. The inherited registry lock stays CLOEXEC until exec or
    // exit, so reconstruction cannot race a child still in this setup boundary.
    unsafe {
        command.pre_exec(move || {
            prepare_child(parent, &manifest, cpu_seconds, address_space)?;
            seccompiler::apply_filter(&program)
                .map_err(|_filter| io::Error::from_raw_os_error(libc::EPERM))?;
            let arguments = [c"penelope-worker".as_ptr(), std::ptr::null()];
            let environment = [std::ptr::null::<libc::c_char>()];
            // SAFETY: all pointers reference live NUL-terminated strings or
            // terminated pointer arrays. The descriptor owns the sealed image.
            // Success replaces the image and closes CLOEXEC descriptors,
            // including Rust's exec-error pipe; failure returns an OS error.
            libc::syscall(
                libc::SYS_execveat,
                executable.as_raw_fd(),
                c"".as_ptr(),
                arguments.as_ptr(),
                environment.as_ptr(),
                libc::AT_EMPTY_PATH,
            );
            Err(io::Error::last_os_error())
        });
    }
    command.spawn().map_err(|_spawn| TaskPortError::Runtime)
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
mod tests;
