//! Bounded task supervision. Each call commits at most one history record.
//! A commit/runtime error poisons the supervisor until reconstruction. Replay
//! never executes user code. Kernel safety is independent of this service.

use super::{
    BlobRef, Digest, Reconciliation, TaskDefinition, TaskError, TaskExecutionEvent, TaskFault,
    TaskGeneration, TaskProjection, TaskRecord, TaskScope, TaskStatus, TaskTick,
    runtime::{
        RuntimeProgress, TaskJournal, TaskObservation, TaskPortError, TaskRuntime, TaskStart,
    },
};
use thiserror::Error;

/// Supervisor failures preserve ambiguity and prevent subsequent dispatch.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorError {
    #[error(transparent)]
    Decision(#[from] TaskError),
    #[error(transparent)]
    Port(#[from] TaskPortError),
    #[error("supervisor requires reconstruction after an ambiguous operation")]
    ReconstructionRequired,
}

/// Caller-owned byte buffers. Each slice exposes its usable capacity; the
/// controller never allocates or reserves hidden input/output/checkpoint arrays.
pub struct TaskWorkspace<'workspace> {
    pub input: &'workspace mut [u8],
    pub output: &'workspace mut [u8],
    pub checkpoint: &'workspace mut [u8],
}

impl TaskWorkspace<'_> {
    /// Checks all declared capacities before dispatch or runtime polling.
    /// # Errors
    /// Returns quota exhaustion without consuming runtime progress or committing.
    pub fn validate(&self, definition: TaskDefinition) -> Result<(), TaskError> {
        validate_capacity(definition.budget.max_input_bytes, self.input.len())?;
        validate_poll_capacity(definition, self.output.len())?;
        validate_capacity(
            definition.recovery.max_checkpoint_bytes,
            self.checkpoint.len(),
        )
    }
}

fn validate_capacity(required: u32, available: usize) -> Result<(), TaskError> {
    if usize::try_from(required)
        .ok()
        .is_none_or(|bytes| bytes > available)
    {
        return Err(TaskError::QuotaExhausted);
    }
    Ok(())
}

fn validate_poll_capacity(definition: TaskDefinition, available: usize) -> Result<(), TaskError> {
    validate_capacity(
        definition
            .budget
            .max_output_bytes
            .max(definition.recovery.max_checkpoint_bytes),
        available,
    )
}

/// Portable supervisor sharing the v1 projection and owning no byte workspace.
pub struct TaskControl<J, R> {
    journal: J,
    runtime: R,
    projection: TaskProjection,
    poisoned: bool,
}

impl<J: TaskJournal, R: TaskRuntime> TaskControl<J, R> {
    /// Loads bounded history. A restored running generation is fenced before a
    /// supervisor-interruption fact is committed; it is never redispatched.
    /// # Errors
    /// Returns malformed history, unavailable storage or unconfirmed fencing.
    pub fn restore(
        mut journal: J,
        mut runtime: R,
        scope: TaskScope,
        definition: TaskDefinition,
        tick: TaskTick,
    ) -> Result<Self, SupervisorError> {
        let mut projection = TaskProjection::new(scope, definition)?;
        while let Some(record) = journal.read(scope, projection.next_sequence())? {
            projection.apply(&record)?;
        }
        if tick < projection.last_tick() {
            return Err(TaskError::InvalidOrder.into());
        }
        if projection.status() == TaskStatus::Running {
            runtime.fence(scope, projection.generation())?;
            let record = projection.propose(
                TaskExecutionEvent::FaultObserved {
                    fault: TaskFault::SupervisorInterrupted,
                },
                projection.generation(),
                tick,
            )?;
            let receipt = journal.commit(&record, None)?;
            if receipt != record {
                return Err(TaskPortError::Corrupt.into());
            }
            projection.apply(&record)?;
        }
        Ok(Self {
            journal,
            runtime,
            projection,
            poisoned: false,
        })
    }

    pub const fn projection(&self) -> &TaskProjection {
        &self.projection
    }

    /// Returns adapters for a new supervisor after interruption/failure.
    pub fn into_parts(self) -> (J, R) {
        (self.journal, self.runtime)
    }

    /// Commits immutable input together with the pinned definition intent.
    /// # Errors
    /// Returns invalid input/policy or storage failures; never starts on error.
    pub fn submit(
        &mut self,
        input: &[u8],
        schema: u32,
        tick: TaskTick,
    ) -> Result<(), SupervisorError> {
        self.ensure_healthy()?;
        let blob = blob_ref(
            input,
            schema,
            self.projection.definition().budget.max_input_bytes,
        )?;
        let record = self.projection.propose(
            TaskExecutionEvent::IntentCommitted {
                definition: self.projection.definition(),
                input: blob,
            },
            TaskGeneration::ZERO,
            tick,
        )?;
        self.commit(&record, Some(input))
    }

    /// Advances one deterministic lifecycle transition. Start occurs only after
    /// its record commits; fault recovery occurs only after runtime fencing.
    /// # Errors
    /// Returns clock/policy, storage, integrity or execution adapter failures.
    pub fn advance(
        &mut self,
        tick: TaskTick,
        workspace: &mut TaskWorkspace<'_>,
    ) -> Result<bool, SupervisorError> {
        self.advance_inner(
            tick,
            Some((&mut *workspace.input, &mut *workspace.checkpoint)),
            Some(workspace.output.len()),
        )
    }

    /// Advances using only dispatch input/checkpoint storage. No poll buffer is
    /// borrowed or initialized by this call. A later `poll_with_buffer` validates
    /// its actual buffer before consuming runtime progress. Use `advance` with a
    /// full workspace when all three capacities must be admitted before launch.
    /// # Errors
    /// Returns clock/policy, storage, integrity or execution adapter failures.
    pub fn advance_with_buffers(
        &mut self,
        tick: TaskTick,
        input: &mut [u8],
        checkpoint: &mut [u8],
    ) -> Result<bool, SupervisorError> {
        self.advance_inner(tick, Some((input, checkpoint)), None)
    }

    fn advance_inner(
        &mut self,
        tick: TaskTick,
        buffers: Option<(&mut [u8], &mut [u8])>,
        poll_capacity: Option<usize>,
    ) -> Result<bool, SupervisorError> {
        self.ensure_healthy()?;
        let Some(record) = self.projection.next(tick)? else {
            return Ok(false);
        };
        let dispatch = if matches!(
            record.event,
            TaskExecutionEvent::Started | TaskExecutionEvent::Restarted
        ) {
            let (input, checkpoint) = buffers.ok_or(TaskError::InvalidTransition)?;
            let definition = self.projection.definition();
            validate_capacity(definition.budget.max_input_bytes, input.len())?;
            validate_capacity(definition.recovery.max_checkpoint_bytes, checkpoint.len())?;
            if let Some(capacity) = poll_capacity {
                validate_poll_capacity(definition, capacity)?;
            }
            Some((input, checkpoint))
        } else {
            None
        };
        if matches!(record.event, TaskExecutionEvent::FaultObserved { .. }) {
            self.fence(record.scope, record.generation)?;
        }
        self.commit(&record, None)?;
        if let Some((input, checkpoint)) = dispatch {
            let result = self.dispatch(input, checkpoint);
            if result.is_err() {
                self.poisoned = true;
            }
            result?;
        }
        Ok(true)
    }

    fn dispatch(&mut self, input: &mut [u8], checkpoint: &mut [u8]) -> Result<(), SupervisorError> {
        let scope = self.projection.scope();
        let input_ref = self
            .projection
            .input()
            .ok_or(TaskError::InvalidTransition)?;
        let input_len = self.journal.read_blob(scope, input_ref, input)?;
        let input_bytes = input.get(..input_len).ok_or(TaskPortError::Corrupt)?;
        input_ref.verify(input_bytes, input_ref.schema)?;
        let resume = if let Some(reference) = self.projection.checkpoint() {
            let len = self.journal.read_blob(scope, reference, checkpoint)?;
            let bytes = checkpoint.get(..len).ok_or(TaskPortError::Corrupt)?;
            reference.verify(bytes, self.projection.definition().checkpoint_schema)?;
            Some(bytes)
        } else {
            None
        };
        self.runtime.start(TaskStart {
            scope,
            generation: self.projection.generation(),
            definition: self.projection.definition(),
            execution_deadline: self.projection.execution_deadline(),
            input_schema: input_ref.schema,
            input: input_bytes,
            checkpoint: resume,
        })?;
        Ok(())
    }

    /// Polls a running generation. Timeout wins over late completion/checkpoint.
    /// # Errors
    /// Rejects stale observations and fails closed on ambiguous runtime/storage.
    pub fn poll(
        &mut self,
        tick: TaskTick,
        workspace: &mut TaskWorkspace<'_>,
    ) -> Result<bool, SupervisorError> {
        self.poll_inner(
            tick,
            workspace.output,
            Some((workspace.input.len(), workspace.checkpoint.len())),
        )
    }

    /// Polls using one buffer sized for both checkpoint and completion proposals.
    /// Input and resume-checkpoint storage are not needed while polling. Timeout
    /// advances the same shared policy without allocating dispatch storage.
    /// # Errors
    /// Rejects insufficient capacity before consuming runtime progress; other
    /// policy, observation and ambiguity errors match `poll`.
    pub fn poll_with_buffer(
        &mut self,
        tick: TaskTick,
        buffer: &mut [u8],
    ) -> Result<bool, SupervisorError> {
        self.poll_inner(tick, buffer, None)
    }

    fn poll_inner(
        &mut self,
        tick: TaskTick,
        buffer: &mut [u8],
        dispatch_capacities: Option<(usize, usize)>,
    ) -> Result<bool, SupervisorError> {
        self.ensure_healthy()?;
        if tick < self.projection.last_tick() {
            return Err(TaskError::InvalidOrder.into());
        }
        if self.projection.status() != TaskStatus::Running {
            return Err(TaskError::InvalidTransition.into());
        }
        if tick >= self.projection.execution_deadline() {
            return self.advance_inner(tick, None, None);
        }
        let definition = self.projection.definition();
        if let Some((input_capacity, checkpoint_capacity)) = dispatch_capacities {
            validate_capacity(definition.budget.max_input_bytes, input_capacity)?;
            validate_capacity(
                definition.recovery.max_checkpoint_bytes,
                checkpoint_capacity,
            )?;
        }
        validate_poll_capacity(definition, buffer.len())?;
        let observation = match self.runtime.poll(
            self.projection.scope(),
            self.projection.generation(),
            buffer,
        ) {
            Ok(observation) => observation,
            Err(error) => {
                self.poisoned = true;
                return Err(error.into());
            }
        };
        self.observe(observation, buffer, tick)
    }

    /// Admits a trusted, scoped observation (including forwarded kernel faults).
    /// Checkpoint bytes must be a valid effect boundary under the application contract.
    /// # Errors
    /// Rejects stale/foreign proposals and bad bytes. A late non-fault proposal
    /// fences execution and records timeout before returning `DeadlineExpired`.
    /// A running generation exceeding admission quotas is fenced and requires
    /// reconstruction; rejected checkpoints are never acknowledged.
    pub fn observe(
        &mut self,
        observation: TaskObservation,
        buffer: &[u8],
        tick: TaskTick,
    ) -> Result<bool, SupervisorError> {
        self.ensure_healthy()?;
        if tick < self.projection.last_tick() {
            return Err(TaskError::InvalidOrder.into());
        }
        if observation.scope != self.projection.scope() {
            return Err(TaskError::ScopeMismatch.into());
        }
        if observation.generation != self.projection.generation() {
            return Err(TaskError::StaleGeneration.into());
        }
        if self.projection.status() != TaskStatus::Running {
            return Err(TaskError::InvalidTransition.into());
        }
        if tick >= self.projection.execution_deadline()
            && !matches!(observation.progress, RuntimeProgress::Fault(_))
        {
            let record = self
                .projection
                .next(tick)?
                .ok_or(TaskError::InvalidTransition)?;
            self.fence(record.scope, record.generation)?;
            self.commit(&record, None)?;
            return Err(TaskError::DeadlineExpired.into());
        }
        let (event, blob) = match observation.progress {
            RuntimeProgress::Pending => return Ok(false),
            RuntimeProgress::Fault(fault) => (TaskExecutionEvent::FaultObserved { fault }, None),
            RuntimeProgress::Checkpoint { bytes, schema }
            | RuntimeProgress::Complete { bytes, schema } => {
                let definition = self.projection.definition();
                let limit = if matches!(observation.progress, RuntimeProgress::Checkpoint { .. }) {
                    definition.recovery.max_checkpoint_bytes
                } else {
                    definition.budget.max_output_bytes
                };
                // Enforce the declared size before inspecting the supplied
                // buffer. An oversized report must stop execution even when
                // the adapter could not supply bytes beyond its own capacity.
                if bytes > limit {
                    return Err(self.reject_live_proposal(TaskError::QuotaExhausted));
                }
                let length = match usize::try_from(bytes) {
                    Ok(length) => length,
                    Err(_overflow) => {
                        return Err(self.reject_live_proposal(TaskError::QuotaExhausted));
                    }
                };
                let data = buffer.get(..length).ok_or(TaskError::CorruptBlob)?;
                let reference = match blob_ref(data, schema, limit) {
                    Ok(reference) => reference,
                    Err(error) => return Err(self.reject_live_proposal(error)),
                };
                let event = if matches!(observation.progress, RuntimeProgress::Checkpoint { .. }) {
                    TaskExecutionEvent::CheckpointCommitted {
                        checkpoint: reference,
                    }
                } else {
                    TaskExecutionEvent::Completed { output: reference }
                };
                (event, Some(data))
            }
        };
        let record = match self.projection.propose(event, observation.generation, tick) {
            Ok(record) => record,
            Err(error) => return Err(self.reject_live_proposal(error)),
        };
        if matches!(
            event,
            TaskExecutionEvent::FaultObserved { .. } | TaskExecutionEvent::Completed { .. }
        ) {
            self.fence(observation.scope, observation.generation)?;
        }
        self.commit(&record, blob)?;
        if let TaskExecutionEvent::CheckpointCommitted { checkpoint } = event {
            let acknowledgement = self.runtime.acknowledge_checkpoint(
                observation.scope,
                observation.generation,
                checkpoint,
            );
            if let Err(error) = acknowledgement {
                self.poisoned = true;
                return Err(error.into());
            }
        }
        Ok(true)
    }

    /// Records independently obtained reconciliation evidence. Completion bytes
    /// commit atomically with the evidence; unknown outcomes remain blocked.
    /// # Errors
    /// Rejects scope/generation mismatch, inconsistent bytes and invalid evidence phase.
    pub fn reconcile(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        result: Reconciliation,
        output: Option<&[u8]>,
        tick: TaskTick,
    ) -> Result<(), SupervisorError> {
        self.ensure_healthy()?;
        if scope != self.projection.scope() {
            return Err(TaskError::ScopeMismatch.into());
        }
        if generation != self.projection.generation() {
            return Err(TaskError::StaleGeneration.into());
        }
        if let Reconciliation::Completed {
            output: reference, ..
        } = result
        {
            if reference.bytes > self.projection.definition().budget.max_output_bytes {
                return Err(TaskError::QuotaExhausted.into());
            }
            reference.verify(output.ok_or(TaskError::CorruptBlob)?, reference.schema)?;
        } else if output.is_some() {
            return Err(TaskError::CorruptBlob.into());
        }
        let record =
            self.projection
                .propose(TaskExecutionEvent::Reconciled { result }, generation, tick)?;
        self.commit(&record, output)
    }

    /// Imports an explicitly transformed checkpoint into a new pinned history.
    /// The application must authorize migration and fence the source execution;
    /// Penelope does not reinterpret or transform application state layouts.
    /// # Errors
    /// Rejects foreign scope, unchanged definitions, schema/size errors and storage failures.
    pub fn import_checkpoint(
        &mut self,
        source: TaskScope,
        previous: BlobRef,
        bytes: &[u8],
        authorization: Digest,
        tick: TaskTick,
    ) -> Result<(), SupervisorError> {
        self.ensure_healthy()?;
        let definition = self.projection.definition();
        let checkpoint = blob_ref(
            bytes,
            definition.checkpoint_schema,
            definition.recovery.max_checkpoint_bytes,
        )?;
        let record = self.projection.propose(
            TaskExecutionEvent::CheckpointMigrated {
                source,
                previous,
                checkpoint,
                authorization,
            },
            TaskGeneration::ZERO,
            tick,
        )?;
        self.commit(&record, Some(bytes))
    }

    fn fence(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
    ) -> Result<(), SupervisorError> {
        match self.runtime.fence(scope, generation) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.poisoned = true;
                Err(error.into())
            }
        }
    }

    fn reject_live_proposal(&mut self, error: TaskError) -> SupervisorError {
        if error == TaskError::QuotaExhausted {
            let fenced = self.fence(self.projection.scope(), self.projection.generation());
            self.poisoned = true;
            if let Err(failure) = fenced {
                return failure;
            }
        }
        error.into()
    }

    const fn ensure_healthy(&self) -> Result<(), SupervisorError> {
        if self.poisoned {
            return Err(SupervisorError::ReconstructionRequired);
        }
        Ok(())
    }

    fn commit(&mut self, record: &TaskRecord, blob: Option<&[u8]>) -> Result<(), SupervisorError> {
        self.ensure_healthy()?;
        match self.journal.commit(record, blob) {
            Ok(receipt) if receipt == *record => self.projection.apply(record).map_err(Into::into),
            Ok(_) => {
                self.poisoned = true;
                Err(TaskPortError::Corrupt.into())
            }
            Err(error) => {
                self.poisoned = true;
                Err(error.into())
            }
        }
    }
}

fn blob_ref(bytes: &[u8], schema: u32, limit: u32) -> Result<BlobRef, TaskError> {
    let length = u32::try_from(bytes.len()).map_err(|_overflow| TaskError::QuotaExhausted)?;
    if length > limit || schema == 0 {
        return Err(TaskError::QuotaExhausted);
    }
    Ok(BlobRef {
        digest: Digest::of(bytes),
        bytes: length,
        schema,
    })
}
