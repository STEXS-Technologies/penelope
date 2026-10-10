//! Synchronous, allocation-free runtime adapter contracts. Implementations must
//! enforce isolation, CPU budgets and fencing outside the decision engine.

use super::{BlobRef, TaskDefinition, TaskFault, TaskRecord, TaskScope};

/// Durable journal failure. Ambiguous commits must be resolved by replay;
/// callers must never dispatch on an error, including a lost acknowledgement.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum TaskPortError {
    #[error("storage unavailable or commit acknowledgement lost")]
    Unavailable,
    #[error("storage quota exhausted")]
    Exhausted,
    #[error("stored bytes or history are corrupt")]
    Corrupt,
    #[error("journal CAS or runtime fence conflict")]
    Conflict,
    #[error("execution adapter failed")]
    Runtime,
}

/// Journal reads must be gap-free, linearizable and scoped. Atomic commit checks
/// exact sequence and generation transition, persists an optional immutable blob
/// with the record, and returns the exact record as acknowledgement. A conflict
/// changes nothing. Ambiguous errors require reconstruction, never blind dispatch.
/// Retain input/output/checkpoints until their history is safely retired.
/// Reads verify the deployment's record-integrity protection and return
/// `Corrupt` on failure. Semantic replay is not record authentication; a digest
/// or checksum alone does not protect against an attacker rewriting the journal.
pub trait TaskJournal {
    /// Reads one record; `None` means the authoritative end, not a read failure.
    /// # Errors
    /// Returns storage availability, integrity or scope failures.
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: u32,
    ) -> Result<Option<TaskRecord>, TaskPortError>;
    /// Atomically persists bytes and their referring event; verifies digest/size.
    /// # Errors
    /// Returns storage errors or a CAS conflict.
    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError>;
    /// Reads exact bytes into caller-owned memory. Never truncates.
    /// # Errors
    /// Returns `Exhausted` if the buffer is too small, or a storage failure.
    fn read_blob(
        &mut self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError>;
}

/// Persisted start authorization passed to a protected runtime. Repeating start
/// for this exact scope/generation must not create a second live execution.
#[derive(Debug, Clone, Copy)]
pub struct TaskStart<'bytes> {
    pub scope: TaskScope,
    pub generation: u32,
    pub definition: TaskDefinition,
    pub execution_deadline: u64,
    pub input: &'bytes [u8],
    pub checkpoint: Option<&'bytes [u8]>,
}

/// Bytes returned by `poll` are proposals in a caller-owned buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProgress {
    Pending,
    Checkpoint { bytes: u32, schema: u32 },
    Complete { bytes: u32, schema: u32 },
    Fault(TaskFault),
}

/// Every runtime observation is fenced by exact scope and generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskObservation {
    pub scope: TaskScope,
    pub generation: u32,
    pub progress: RuntimeProgress,
}

/// Runtime owns scheduling/protection; Penelope authorizes recovery. `fence`
/// must stop the generation and prevent further execution/effect issuance before
/// acknowledging. Already-issued effects can still complete; they require
/// independent reconciliation and cannot be retracted by a runtime fence.
/// OS watchdog and immediate physical safety responses remain independent.
/// Checkpoint-producing workers pause until acknowledgement or fencing.
pub trait TaskRuntime {
    /// Starts/resumes only the exact durably authorized generation.
    /// # Errors
    /// Returns execution or fence failure; an error can be an ambiguous start.
    fn start(&mut self, request: TaskStart<'_>) -> Result<(), TaskPortError>;
    /// Polls proposals without allocating or blocking on task execution. The
    /// adapter enforces execution deadlines/CPU limits and returns trusted facts.
    /// Caller persists before acknowledgement.
    /// # Errors
    /// Returns runtime failures; buffer exhaustion must never truncate a proposal.
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: u32,
        buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError>;
    /// Acknowledges the exact checkpoint after its durable commit.
    /// # Errors
    /// Returns runtime failures; recovery must replay the already committed boundary.
    fn acknowledge_checkpoint(
        &mut self,
        scope: TaskScope,
        generation: u32,
        checkpoint: BlobRef,
    ) -> Result<(), TaskPortError>;
    /// Stops and fences a generation. Never restart if fencing cannot be proven.
    /// # Errors
    /// Returns `Conflict` or a runtime failure when termination is unconfirmed.
    fn fence(&mut self, scope: TaskScope, generation: u32) -> Result<(), TaskPortError>;
}

/// Monotonic clock comes from the platform, never wall-clock timestamps.
pub trait TaskClock {
    fn now(&self) -> u64;
}
