//! Test-only adapters: no storage/runtime implementation enters production.
// Shared by different harnesses; each intentionally uses a subset of adapters.
#![allow(dead_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::panic
)]
use penelope_core::task::runtime::*;
use penelope_core::task::*;
use std::collections::BTreeMap;

pub fn definition(mode: RecoveryMode, effects: EffectContract) -> TaskDefinition {
    TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects,
        recovery: TaskRecoveryPolicy {
            mode,
            max_attempts: 32,
            timeout_ticks: 50,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 64,
        },
        budget: ExecutionBudget {
            max_input_bytes: 64,
            max_output_bytes: 64,
            max_history_records: 512,
            max_checkpoint_commits: 64,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(10000),
        },
    }
}

pub fn scope(definition: TaskDefinition, input: &[u8]) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(input)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    None,
    BeforeCommit,
    AfterCommit,
    WrongReceipt,
    StorageFull,
}

#[derive(Clone, Default)]
pub struct MemoryJournal {
    pub records: Vec<TaskRecord>,
    pub blobs: BTreeMap<[u8; 32], Vec<u8>>,
    pub cut: Option<Cut>,
}

impl TaskJournal for MemoryJournal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        let result = self.records.get(sequence.get() as usize).copied();
        if result.is_some_and(|record| record.scope != scope) {
            return Err(TaskPortError::Conflict);
        }
        Ok(result)
    }
    fn commit(
        &mut self,
        record: &TaskRecord,
        bytes: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        let cut = self.cut.take().unwrap_or(Cut::None);
        if cut == Cut::BeforeCommit {
            return Err(TaskPortError::Unavailable);
        }
        if cut == Cut::StorageFull {
            return Err(TaskPortError::Exhausted);
        }
        if record.sequence.get() as usize != self.records.len() {
            return Err(TaskPortError::Conflict);
        }
        let references = match record.event {
            TaskExecutionEvent::IntentCommitted { input, .. } => Some(input),
            TaskExecutionEvent::CheckpointCommitted { checkpoint }
            | TaskExecutionEvent::CheckpointMigrated { checkpoint, .. } => Some(checkpoint),
            TaskExecutionEvent::Completed { output }
            | TaskExecutionEvent::Reconciled {
                result: Reconciliation::Completed { output, .. },
            } => Some(output),
            TaskExecutionEvent::Started
            | TaskExecutionEvent::FaultObserved { .. }
            | TaskExecutionEvent::ReconciliationRequired
            | TaskExecutionEvent::Reconciled { .. }
            | TaskExecutionEvent::RecoveryPlanned { .. }
            | TaskExecutionEvent::Restarted
            | TaskExecutionEvent::RecoveryExhausted => None,
        };
        match (references, bytes) {
            (Some(reference), Some(data)) => {
                reference
                    .verify(data, reference.schema)
                    .map_err(|_invalid| TaskPortError::Corrupt)?;
                self.blobs.insert(reference.digest.0, data.to_vec());
            }
            (None, None) => {}
            _ => return Err(TaskPortError::Corrupt),
        }
        self.records.push(*record);
        if cut == Cut::AfterCommit {
            return Err(TaskPortError::Unavailable);
        }
        let mut receipt = *record;
        if cut == Cut::WrongReceipt {
            receipt.generation = receipt.generation.saturating_add(1);
        }
        Ok(receipt)
    }
    fn read_blob(
        &mut self,
        _scope: TaskScope,
        reference: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        let data = self
            .blobs
            .get(&reference.digest.0)
            .ok_or(TaskPortError::Corrupt)?;
        let dst = buffer
            .get_mut(..data.len())
            .ok_or(TaskPortError::Exhausted)?;
        dst.copy_from_slice(data);
        Ok(data.len())
    }
}

/// Customer computation is separate from the supervision/runtime adapter.
pub struct SumTask;
impl RecoverableTask for SumTask {
    type Input = Vec<u8>;
    type Checkpoint = (u32, u64);
    type Output = u64;
    type Error = ();
    fn execute(
        &mut self,
        input: &Self::Input,
        checkpoint: Option<&Self::Checkpoint>,
    ) -> TaskProgress<Self::Checkpoint, Self::Output, Self::Error> {
        let (offset, mut sum) = checkpoint.copied().unwrap_or((0, 0));
        if offset as usize > input.len() {
            return TaskProgress::Failed(());
        }
        if offset as usize == input.len() {
            return TaskProgress::Complete(sum);
        }
        let end = (offset as usize + 8).min(input.len());
        for byte in &input[offset as usize..end] {
            sum += u64::from(*byte);
        }
        TaskProgress::Checkpoint((end as u32, sum))
    }
}

/// Summation resumes from an encoded offset and sum, never from a Rust stack.
#[derive(Clone, Default)]
pub struct SumRuntime {
    pub starts: u32,
    pub fences: u32,
    pub resumed: Vec<u32>,
    pub fault: Option<TaskFault>,
    pub fence_failure: bool,
    pub acknowledge_failure: bool,
    pub stale: bool,
    input: Vec<u8>,
    offset: u32,
    sum: u64,
    active: bool,
    paused: bool,
}

impl TaskRuntime for SumRuntime {
    fn start(&mut self, request: TaskStart<'_>) -> Result<(), TaskPortError> {
        if request.input_schema != 1 {
            return Err(TaskPortError::Corrupt);
        }
        if self.active {
            return Err(TaskPortError::Conflict);
        }
        self.starts += 1;
        self.input = request.input.to_vec();
        if let Some(bytes) = request.checkpoint {
            self.offset = u32::from_le_bytes(
                bytes
                    .get(..4)
                    .ok_or(TaskPortError::Corrupt)?
                    .try_into()
                    .map_err(|_length| TaskPortError::Corrupt)?,
            );
            self.sum = u64::from_le_bytes(
                bytes
                    .get(4..12)
                    .ok_or(TaskPortError::Corrupt)?
                    .try_into()
                    .map_err(|_length| TaskPortError::Corrupt)?,
            );
        } else {
            self.offset = 0;
            self.sum = 0;
        }
        if self.offset as usize > self.input.len() {
            return Err(TaskPortError::Corrupt);
        }
        self.resumed.push(self.offset);
        self.active = true;
        self.paused = false;
        Ok(())
    }
    fn poll(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        buffer: &mut [u8],
    ) -> Result<TaskObservation, TaskPortError> {
        if !self.active || self.paused {
            return Err(TaskPortError::Runtime);
        }
        let progress = if let Some(fault) = self.fault.take() {
            RuntimeProgress::Fault(fault)
        } else {
            match SumTask.execute(&self.input, Some(&(self.offset, self.sum))) {
                TaskProgress::Complete(sum) => {
                    buffer[..8].copy_from_slice(&sum.to_le_bytes());
                    RuntimeProgress::Complete {
                        bytes: 8,
                        schema: 1,
                    }
                }
                TaskProgress::Checkpoint((offset, sum)) => {
                    self.offset = offset;
                    self.sum = sum;
                    buffer[..4].copy_from_slice(&offset.to_le_bytes());
                    buffer[4..12].copy_from_slice(&sum.to_le_bytes());
                    self.paused = true;
                    RuntimeProgress::Checkpoint {
                        bytes: 12,
                        schema: 1,
                    }
                }
                TaskProgress::Failed(()) => RuntimeProgress::Fault(TaskFault::ApplicationFailure),
            }
        };
        Ok(TaskObservation {
            scope,
            generation: if self.stale {
                generation.saturating_sub(1)
            } else {
                generation
            },
            progress,
        })
    }
    fn acknowledge_checkpoint(
        &mut self,
        _scope: TaskScope,
        _generation: TaskGeneration,
        _checkpoint: BlobRef,
    ) -> Result<(), TaskPortError> {
        if self.acknowledge_failure {
            return Err(TaskPortError::Runtime);
        }
        self.paused = false;
        Ok(())
    }
    fn fence(
        &mut self,
        _scope: TaskScope,
        _generation: TaskGeneration,
    ) -> Result<(), TaskPortError> {
        if self.fence_failure {
            return Err(TaskPortError::Conflict);
        }
        self.active = false;
        self.paused = false;
        self.fences += 1;
        Ok(())
    }
}
