use penelope_core::task::{Digest, runtime::TaskPortError};
use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub(crate) struct ProgramImage {
    pub(crate) file: File,
    pub(crate) digest: Digest,
}
impl ProgramImage {
    pub(crate) fn load(path: &Path, max_bytes: u64) -> Result<Self, TaskPortError> {
        if max_bytes == 0 || max_bytes > 64 * 1024 * 1024 {
            return Err(TaskPortError::Exhausted);
        }
        // Nonblocking open avoids waiting for a FIFO peer before its type is
        // known. Validate the opened descriptor so path replacement cannot
        // substitute a different file between the check and the read.
        let source = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .map_err(|_open| TaskPortError::Runtime)?;
        if !source
            .metadata()
            .map_err(|_stat| TaskPortError::Runtime)?
            .is_file()
        {
            return Err(TaskPortError::Corrupt);
        }
        let mut bytes = Vec::new();
        source
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_read| TaskPortError::Runtime)?;
        if u64::try_from(bytes.len())
            .ok()
            .is_none_or(|size| size > max_bytes)
        {
            return Err(TaskPortError::Exhausted);
        }
        verify_static_elf(&bytes)?;
        let fd = memfd_create(
            c"penelope-worker",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING | MemfdFlags::EXEC,
        )
        .map_err(|_memfd| TaskPortError::Runtime)?;
        let mut file = File::from(fd);
        file.write_all(&bytes)
            .map_err(|_write| TaskPortError::Runtime)?;
        fcntl_add_seals(
            &file,
            SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
        )
        .map_err(|_seal| TaskPortError::Runtime)?;
        Ok(Self {
            file,
            digest: Digest::of(&bytes),
        })
    }
}
fn word<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], TaskPortError> {
    let end = offset.checked_add(N).ok_or(TaskPortError::Corrupt)?;
    bytes
        .get(offset..end)
        .ok_or(TaskPortError::Corrupt)?
        .try_into()
        .map_err(|_length| TaskPortError::Corrupt)
}
fn verify_static_elf(bytes: &[u8]) -> Result<(), TaskPortError> {
    if bytes.get(..7) != Some(b"\x7fELF\x02\x01\x01") || u16::from_le_bytes(word(bytes, 18)?) != 62
    {
        return Err(TaskPortError::Corrupt);
    }
    if !matches!(u16::from_le_bytes(word(bytes, 16)?), 2 | 3) {
        return Err(TaskPortError::Corrupt);
    }
    let offset = usize::try_from(u64::from_le_bytes(word(bytes, 32)?))
        .map_err(|_overflow| TaskPortError::Corrupt)?;
    let size = usize::from(u16::from_le_bytes(word(bytes, 54)?));
    let count = usize::from(u16::from_le_bytes(word(bytes, 56)?));
    if size != 56 || count == 0 || count > 128 {
        return Err(TaskPortError::Corrupt);
    }
    for index in 0..count {
        let start = offset
            .checked_add(index.checked_mul(size).ok_or(TaskPortError::Corrupt)?)
            .ok_or(TaskPortError::Corrupt)?;
        let entry = bytes
            .get(start..start.checked_add(size).ok_or(TaskPortError::Corrupt)?)
            .ok_or(TaskPortError::Corrupt)?;
        if u32::from_le_bytes(word(entry, 0)?) == 3 {
            return Err(TaskPortError::Runtime);
        }
    }
    Ok(())
}
