use crate::{
    sandbox::{IDENTITY_OFFSET, ProcessIdentity},
    worker::scope_digest,
};
use penelope_core::task::{Digest, TaskGeneration, TaskScope, runtime::TaskPortError};
use rustix::fs::{FlockOperation, flock};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) struct Registry {
    root: PathBuf,
    pub(crate) boot: Digest,
    timeout: Duration,
}
pub(crate) struct RegistryGuard {
    _lock: File,
}
pub(crate) struct SavedExecution {
    pub(crate) identity: Option<ProcessIdentity>,
    pub(crate) boot: Digest,
}
fn filename(execution: Digest) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = String::with_capacity(64);
    for byte in execution.0 {
        name.push(char::from(
            *HEX.get(usize::from(byte >> 4)).unwrap_or(&b'0'),
        ));
        name.push(char::from(
            *HEX.get(usize::from(byte & 15)).unwrap_or(&b'0'),
        ));
    }
    name
}
fn raw<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], TaskPortError> {
    bytes
        .get(offset..offset.checked_add(N).ok_or(TaskPortError::Corrupt)?)
        .ok_or(TaskPortError::Corrupt)?
        .try_into()
        .map_err(|_size| TaskPortError::Corrupt)
}
fn header(
    scope: TaskScope,
    generation: TaskGeneration,
    execution: Digest,
    boot: Digest,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(172);
    bytes.extend_from_slice(b"PNLREG01");
    bytes.extend_from_slice(&execution.0);
    bytes.extend_from_slice(&scope_digest(scope).0);
    bytes.extend_from_slice(&generation.get().to_be_bytes());
    bytes.extend_from_slice(&scope.definition.digest().0);
    bytes.extend_from_slice(&boot.0);
    bytes.resize(172, 0);
    bytes
}
impl Registry {
    pub(crate) fn open(root: &Path, timeout: Duration) -> Result<Self, TaskPortError> {
        if timeout.is_zero() || timeout > Duration::from_secs(30) {
            return Err(TaskPortError::Exhausted);
        }
        std::fs::create_dir_all(root).map_err(|_mkdir| TaskPortError::Runtime)?;
        let metadata = std::fs::symlink_metadata(root).map_err(|_stat| TaskPortError::Runtime)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(TaskPortError::Corrupt);
        }
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
            .map_err(|_chmod| TaskPortError::Runtime)?;
        let mut boot = Vec::new();
        File::open("/proc/sys/kernel/random/boot_id")
            .map_err(|_open| TaskPortError::Runtime)?
            .take(128)
            .read_to_end(&mut boot)
            .map_err(|_read| TaskPortError::Runtime)?;
        if boot.len() != 37 {
            return Err(TaskPortError::Runtime);
        }
        Ok(Self {
            root: root.to_path_buf(),
            boot: Digest::of(&boot),
            timeout,
        })
    }
    pub(crate) fn lock(&self) -> Result<RegistryGuard, TaskPortError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(self.root.join("registry.lock"))
            .map_err(|_open| TaskPortError::Runtime)?;
        if !file
            .metadata()
            .map_err(|_stat| TaskPortError::Runtime)?
            .is_file()
        {
            return Err(TaskPortError::Corrupt);
        }
        let deadline = Instant::now()
            .checked_add(self.timeout)
            .ok_or(TaskPortError::Runtime)?;
        loop {
            match flock(&file, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(RegistryGuard { _lock: file }),
                Err(error) if error == rustix::io::Errno::WOULDBLOCK => {
                    if Instant::now() >= deadline {
                        return Err(TaskPortError::Conflict);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_lock) => return Err(TaskPortError::Runtime),
            }
        }
    }
    pub(crate) fn prepare(
        &self,
        scope: TaskScope,
        generation: TaskGeneration,
        execution: Digest,
    ) -> Result<File, TaskPortError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(self.root.join(filename(execution)))
            .map_err(|_open| TaskPortError::Conflict)?;
        file.write_all(&header(scope, generation, execution, self.boot))
            .and_then(|()| file.sync_all())
            .map_err(|_write| TaskPortError::Runtime)?;
        File::open(&self.root)
            .and_then(|root| root.sync_all())
            .map_err(|_sync| TaskPortError::Runtime)?;
        Ok(file)
    }
    pub(crate) fn load(
        &self,
        scope: TaskScope,
        generation: TaskGeneration,
        execution: Digest,
    ) -> Result<Option<SavedExecution>, TaskPortError> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(self.root.join(filename(execution)))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_open) => return Err(TaskPortError::Runtime),
        };
        if !file
            .metadata()
            .map_err(|_stat| TaskPortError::Runtime)?
            .is_file()
        {
            return Err(TaskPortError::Corrupt);
        }
        let mut bytes = Vec::new();
        file.take(173)
            .read_to_end(&mut bytes)
            .map_err(|_read| TaskPortError::Runtime)?;
        if bytes.len() != 172 {
            return Err(TaskPortError::Corrupt);
        }
        let boot = Digest(raw(&bytes, 108)?);
        let expected = header(scope, generation, execution, boot);
        let offset =
            usize::try_from(IDENTITY_OFFSET).map_err(|_overflow| TaskPortError::Corrupt)?;
        if bytes.get(..offset) != expected.get(..offset) {
            return Err(TaskPortError::Corrupt);
        }
        let pid = u32::from_be_bytes(raw(&bytes, offset)?);
        let start_ticks = u64::from_be_bytes(raw(&bytes, offset.saturating_add(4))?);
        let identity = match (pid, start_ticks) {
            (0, 0) => None,
            (pid, start_ticks) if pid > 0 && start_ticks > 0 => {
                Some(ProcessIdentity { pid, start_ticks })
            }
            _ => return Err(TaskPortError::Corrupt),
        };
        Ok(Some(SavedExecution { identity, boot }))
    }
}
