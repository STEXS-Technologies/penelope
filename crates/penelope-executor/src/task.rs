//! Bounded task supervision. Each call commits at most one history record.
//! A commit/runtime error poisons the supervisor until reconstruction. Replay
//! never executes user code. Kernel safety is independent of this service.

use penelope_core::task::{
    BlobRef, Digest, Reconciliation, TaskDefinition, TaskError, TaskExecutionEvent, TaskFault,
    TaskProjection, TaskRecord, TaskScope, TaskStatus,
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

/// Fixed-capacity supervised task. Buffers live in the isolated reliability
/// service; `N` bounds each input/checkpoint/result, not the protected task heap.
pub struct TaskSupervisor<J, R, const N: usize> {
    journal: J,
    runtime: R,
    projection: TaskProjection,
    poisoned: bool,
}

impl<J: TaskJournal, R: TaskRuntime, const N: usize> TaskSupervisor<J, R, N> {
    /// Loads bounded history. A restored running generation is fenced before a
    /// supervisor-interruption fact is committed; it is never redispatched.
    /// # Errors
    /// Returns malformed history, unavailable storage or unconfirmed fencing.
    pub fn restore(
        mut journal: J,
        mut runtime: R,
        scope: TaskScope,
        definition: TaskDefinition,
        tick: u64,
    ) -> Result<Self, SupervisorError> {
        let mut projection = TaskProjection::new(scope, definition)?;
        if usize::try_from(definition.budget.max_input_bytes)
            .ok()
            .is_none_or(|n| n > N)
            || usize::try_from(definition.budget.max_output_bytes)
                .ok()
                .is_none_or(|n| n > N)
            || usize::try_from(definition.recovery.max_checkpoint_bytes)
                .ok()
                .is_none_or(|n| n > N)
        {
            return Err(TaskError::QuotaExhausted.into());
        }
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
    pub fn submit(&mut self, input: &[u8], schema: u32, tick: u64) -> Result<(), SupervisorError> {
        let blob = blob_ref(input, schema)?;
        let record = self.projection.propose(
            TaskExecutionEvent::IntentCommitted {
                definition: self.projection.definition(),
                input: blob,
            },
            0,
            tick,
        )?;
        self.commit(&record, Some(input))
    }

    /// Advances one deterministic lifecycle transition. Start occurs only after
    /// its record commits; fault recovery occurs only after runtime fencing.
    /// # Errors
    /// Returns clock/policy, storage, integrity or execution adapter failures.
    pub fn advance(&mut self, tick: u64) -> Result<bool, SupervisorError> {
        self.ensure_healthy()?;
        let Some(record) = self.projection.next(tick)? else {
            return Ok(false);
        };
        if matches!(record.event, TaskExecutionEvent::FaultObserved { .. }) {
            self.fence(record.scope, record.generation)?;
        }
        self.commit(&record, None)?;
        if matches!(
            record.event,
            TaskExecutionEvent::Started | TaskExecutionEvent::Restarted
        ) {
            let result = self.dispatch();
            if result.is_err() {
                self.poisoned = true;
            }
            result?;
        }
        Ok(true)
    }

    fn dispatch(&mut self) -> Result<(), SupervisorError> {
        let mut input = [0; N];
        let mut checkpoint = [0; N];
        let scope = self.projection.scope();
        let input_ref = self
            .projection
            .input()
            .ok_or(TaskError::InvalidTransition)?;
        let input_len = self.journal.read_blob(scope, input_ref, &mut input)?;
        let input_bytes = input.get(..input_len).ok_or(TaskPortError::Corrupt)?;
        input_ref.verify(input_bytes, input_ref.schema)?;
        let resume = if let Some(reference) = self.projection.checkpoint() {
            let len = self.journal.read_blob(scope, reference, &mut checkpoint)?;
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
            input: input_bytes,
            checkpoint: resume,
        })?;
        Ok(())
    }

    /// Polls a running generation. Timeout wins over late completion/checkpoint.
    /// # Errors
    /// Rejects stale observations and fails closed on ambiguous runtime/storage.
    pub fn poll(&mut self, tick: u64) -> Result<bool, SupervisorError> {
        self.ensure_healthy()?;
        if self.projection.status() != TaskStatus::Running {
            return Err(TaskError::InvalidTransition.into());
        }
        if tick >= self.projection.execution_deadline() {
            return self.advance(tick);
        }
        let mut buffer = [0; N];
        let observation = match self.runtime.poll(
            self.projection.scope(),
            self.projection.generation(),
            &mut buffer,
        ) {
            Ok(observation) => observation,
            Err(error) => {
                self.poisoned = true;
                return Err(error.into());
            }
        };
        self.observe(observation, &buffer, tick)
    }

    /// Admits a trusted, scoped observation (including forwarded kernel faults).
    /// Checkpoint bytes must be a valid effect boundary under the application contract.
    /// # Errors
    /// Rejects stale/foreign proposals, bad bytes and late results.
    pub fn observe(
        &mut self,
        observation: TaskObservation,
        buffer: &[u8],
        tick: u64,
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
        let (event, blob) = match observation.progress {
            RuntimeProgress::Pending => return Ok(false),
            RuntimeProgress::Fault(fault) => (TaskExecutionEvent::FaultObserved { fault }, None),
            RuntimeProgress::Checkpoint { bytes, schema }
            | RuntimeProgress::Complete { bytes, schema } => {
                let length =
                    usize::try_from(bytes).map_err(|_overflow| TaskError::QuotaExhausted)?;
                let data = buffer.get(..length).ok_or(TaskError::CorruptBlob)?;
                let reference = blob_ref(data, schema)?;
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
        let record = self
            .projection
            .propose(event, observation.generation, tick)?;
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
        generation: u32,
        result: Reconciliation,
        output: Option<&[u8]>,
        tick: u64,
    ) -> Result<(), SupervisorError> {
        self.ensure_healthy()?;
        if scope != self.projection.scope() {
            return Err(TaskError::ScopeMismatch.into());
        }
        if let Reconciliation::Completed {
            output: reference, ..
        } = result
        {
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
        tick: u64,
    ) -> Result<(), SupervisorError> {
        let checkpoint = blob_ref(bytes, self.projection.definition().checkpoint_schema)?;
        let record = self.projection.propose(
            TaskExecutionEvent::CheckpointMigrated {
                source,
                previous,
                checkpoint,
                authorization,
            },
            0,
            tick,
        )?;
        self.commit(&record, Some(bytes))
    }

    fn fence(&mut self, scope: TaskScope, generation: u32) -> Result<(), SupervisorError> {
        match self.runtime.fence(scope, generation) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.poisoned = true;
                Err(error.into())
            }
        }
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

fn blob_ref(bytes: &[u8], schema: u32) -> Result<BlobRef, TaskError> {
    Ok(BlobRef {
        digest: Digest::of(bytes),
        bytes: u32::try_from(bytes.len()).map_err(|_overflow| TaskError::QuotaExhausted)?,
        schema,
    })
}
