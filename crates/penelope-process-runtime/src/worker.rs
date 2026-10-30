//! Versioned worker IPC. The parent validates every envelope and blob; a worker
//! report is a proposal, never independent proof of application/effect success.
use penelope_core::task::{
    Digest, TaskDefinition, TaskGeneration, TaskScope, TaskTick, runtime::TaskPortError,
};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub(crate) const START_MAGIC: &[u8; 8] = b"PNLWRK01";
pub(crate) const OBS_MAGIC: &[u8; 8] = b"PNLOBS01";
pub(crate) const OBS_HEADER: usize = 53;

/// Full immutable start metadata, followed by input and optional checkpoint bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerMetadata {
    pub scope: TaskScope,
    pub generation: TaskGeneration,
    pub definition: TaskDefinition,
    pub execution_deadline: TaskTick,
    pub input_schema: u32,
}
/// Bounded worker start packet. All lengths are checked before payload allocation.
pub struct WorkerRequest {
    pub metadata: WorkerMetadata,
    pub input: Vec<u8>,
    pub checkpoint: Option<Vec<u8>>,
}

pub(crate) fn scope_digest(scope: TaskScope) -> Digest {
    let mut bytes = Vec::with_capacity(160);
    bytes.extend_from_slice(b"penelope.linux-worker.scope.v1\0");
    for digest in [
        scope.tenant.digest(),
        scope.task.digest(),
        scope.definition.digest(),
        scope.input.digest(),
    ] {
        bytes.extend_from_slice(&digest.0);
    }
    Digest::of(&bytes)
}
/// Reads one immutable request from the parent's private pipe.
/// # Errors
/// Rejects incompatible, truncated, oversized or incorrectly scoped input.
pub fn read_request(
    reader: &mut impl Read,
    max_input: u32,
    max_checkpoint: u32,
) -> Result<WorkerRequest, TaskPortError> {
    let mut header = [0; 60];
    reader
        .read_exact(&mut header)
        .map_err(|_read| TaskPortError::Runtime)?;
    if header.get(..8) != Some(START_MAGIC) {
        return Err(TaskPortError::Corrupt);
    }
    let number = |start: usize| -> Result<u32, TaskPortError> {
        Ok(u32::from_be_bytes(
            header
                .get(start..start.saturating_add(4))
                .ok_or(TaskPortError::Corrupt)?
                .try_into()
                .map_err(|_length| TaskPortError::Corrupt)?,
        ))
    };
    let metadata_len = number(44)?;
    let input_len = number(48)?;
    let checkpoint_len = number(52)?;
    let has_checkpoint = number(56)?;
    if metadata_len > 8192
        || input_len > max_input
        || checkpoint_len > max_checkpoint
        || has_checkpoint > 1
        || (has_checkpoint == 0 && checkpoint_len != 0)
    {
        return Err(TaskPortError::Exhausted);
    }
    let read = |pipe_reader: &mut dyn Read, length: u32| -> Result<Vec<u8>, TaskPortError> {
        let mut bytes =
            vec![0; usize::try_from(length).map_err(|_overflow| TaskPortError::Exhausted)?];
        pipe_reader
            .read_exact(&mut bytes)
            .map_err(|_read| TaskPortError::Runtime)?;
        Ok(bytes)
    };
    let metadata: WorkerMetadata = serde_json::from_slice(&read(reader, metadata_len)?)
        .map_err(|_decode| TaskPortError::Corrupt)?;
    if header.get(8..40) != Some(scope_digest(metadata.scope).0.as_slice())
        || number(40)? != metadata.generation.get()
        || metadata.scope.definition != metadata.definition.fingerprint()
    {
        return Err(TaskPortError::Corrupt);
    }
    metadata
        .definition
        .validate()
        .map_err(|_invalid| TaskPortError::Corrupt)?;
    let input = read(reader, input_len)?;
    if Digest::of(&input) != metadata.scope.input.digest()
        || input_len > metadata.definition.budget.max_input_bytes
        || checkpoint_len > metadata.definition.recovery.max_checkpoint_bytes
        || metadata.input_schema == 0
    {
        return Err(TaskPortError::Corrupt);
    }
    let checkpoint = if has_checkpoint == 1 {
        Some(read(reader, checkpoint_len)?)
    } else {
        None
    };
    Ok(WorkerRequest {
        metadata,
        input,
        checkpoint,
    })
}
/// Writes a scoped completion/checkpoint proposal. A checkpoint worker must call
/// `wait_checkpoint_ack` before resuming computation or producing another frame.
/// # Errors
/// Returns size, schema or pipe failures.
pub fn write_proposal(
    writer: &mut impl Write,
    metadata: &WorkerMetadata,
    checkpoint: bool,
    schema: u32,
    bytes: &[u8],
) -> Result<(), TaskPortError> {
    let limit = if checkpoint {
        metadata.definition.recovery.max_checkpoint_bytes
    } else {
        metadata.definition.budget.max_output_bytes
    };
    let length = u32::try_from(bytes.len()).map_err(|_overflow| TaskPortError::Exhausted)?;
    if length > limit || schema == 0 {
        return Err(TaskPortError::Exhausted);
    }
    writer
        .write_all(OBS_MAGIC)
        .and_then(|()| writer.write_all(&scope_digest(metadata.scope).0))
        .and_then(|()| writer.write_all(&metadata.generation.get().to_be_bytes()))
        .and_then(|()| writer.write_all(&[if checkpoint { 2 } else { 1 }]))
        .and_then(|()| writer.write_all(&length.to_be_bytes()))
        .and_then(|()| writer.write_all(&schema.to_be_bytes()))
        .and_then(|()| writer.write_all(bytes))
        .and_then(|()| writer.flush())
        .map_err(|_write| TaskPortError::Runtime)
}
/// Waits for the exact durable checkpoint acknowledgement from the parent.
/// # Errors
/// Rejects truncated or substituted acknowledgements.
pub fn wait_checkpoint_ack(
    reader: &mut impl Read,
    checkpoint: Digest,
) -> Result<(), TaskPortError> {
    let mut bytes = [0; 40];
    reader
        .read_exact(&mut bytes)
        .map_err(|_read| TaskPortError::Runtime)?;
    if bytes.get(..8) != Some(b"PNLACK01") || bytes.get(8..) != Some(checkpoint.0.as_slice()) {
        return Err(TaskPortError::Corrupt);
    }
    Ok(())
}
