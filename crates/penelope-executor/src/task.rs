//! Hosted compatibility wrapper over the allocation-free shared task controller.

pub use penelope_core::task::control::SupervisorError;
use penelope_core::task::{
    BlobRef, Digest, Reconciliation, TaskDefinition, TaskError, TaskGeneration, TaskProjection,
    TaskScope, TaskTick,
    control::{TaskControl, TaskDispatchWorkspace},
    runtime::{TaskJournal, TaskObservation, TaskRuntime},
};

/// Fixed-capacity supervised task. `N` bounds each input/checkpoint/result.
/// Portable callers can use `TaskControl` with caller-owned `TaskWorkspace`.
pub struct TaskSupervisor<J, R, const N: usize> {
    control: TaskControl<J, R>,
}

impl<J: TaskJournal, R: TaskRuntime, const N: usize> TaskSupervisor<J, R, N> {
    /// Reconstructs history and fences any interrupted generation.
    /// # Errors
    /// Returns invalid capacity/history, storage failures or unconfirmed fencing.
    pub fn restore(
        journal: J,
        runtime: R,
        scope: TaskScope,
        definition: TaskDefinition,
        tick: TaskTick,
    ) -> Result<Self, SupervisorError> {
        if [
            definition.budget.max_input_bytes,
            definition.budget.max_output_bytes,
            definition.recovery.max_checkpoint_bytes,
        ]
        .into_iter()
        .any(|n| usize::try_from(n).ok().is_none_or(|n| n > N))
        {
            return Err(TaskError::QuotaExhausted.into());
        }
        Ok(Self {
            control: TaskControl::restore(journal, runtime, scope, definition, tick)?,
        })
    }

    pub const fn projection(&self) -> &TaskProjection {
        self.control.projection()
    }

    /// Returns adapters for reconstruction.
    pub fn into_parts(self) -> (J, R) {
        self.control.into_parts()
    }

    /// Commits immutable input and intent before execution.
    /// # Errors
    /// Returns invalid input, policy or ambiguous storage failures.
    pub fn submit(
        &mut self,
        input: &[u8],
        schema: u32,
        tick: TaskTick,
    ) -> Result<(), SupervisorError> {
        self.control.submit(input, schema, tick)
    }

    /// Advances one transition, initializing two buffers only when dispatching.
    /// # Errors
    /// Returns policy, storage, integrity or runtime failures.
    pub fn advance(&mut self, tick: TaskTick) -> Result<bool, SupervisorError> {
        self.control
            .advance_with_workspace(tick, &mut FixedDispatchWorkspace::<N>)
    }

    /// Polls with one proposal buffer; timeout wins over completion.
    /// # Errors
    /// Returns stale observations or ambiguous runtime/storage failures.
    // Keep the capacity-sized array out of callers' long-lived stack frames.
    #[inline(never)]
    pub fn poll(&mut self, tick: TaskTick) -> Result<bool, SupervisorError> {
        let mut output = [0; N];
        self.control.poll_with_buffer(tick, &mut output)
    }

    /// Admits a trusted scoped observation and commits before acknowledgement.
    /// # Errors
    /// Rejects stale/foreign observations, invalid bytes and expired deadlines.
    pub fn observe(
        &mut self,
        observation: TaskObservation,
        buffer: &[u8],
        tick: TaskTick,
    ) -> Result<bool, SupervisorError> {
        self.control.observe(observation, buffer, tick)
    }

    /// Commits independently obtained reconciliation evidence.
    /// # Errors
    /// Rejects inconsistent scope, generation, evidence or output bytes.
    pub fn reconcile(
        &mut self,
        scope: TaskScope,
        generation: TaskGeneration,
        result: Reconciliation,
        output: Option<&[u8]>,
        tick: TaskTick,
    ) -> Result<(), SupervisorError> {
        self.control
            .reconcile(scope, generation, result, output, tick)
    }

    /// Imports an explicitly authorized, transformed checkpoint.
    /// # Errors
    /// Rejects incompatible scope, schema, capacity or storage failures.
    pub fn import_checkpoint(
        &mut self,
        source: TaskScope,
        previous: BlobRef,
        bytes: &[u8],
        authorization: Digest,
        tick: TaskTick,
    ) -> Result<(), SupervisorError> {
        self.control
            .import_checkpoint(source, previous, bytes, authorization, tick)
    }
}

struct FixedDispatchWorkspace<const N: usize>;

impl<const N: usize> TaskDispatchWorkspace for FixedDispatchWorkspace<N> {
    // Keep dispatch storage scoped to this call rather than its caller.
    #[inline(never)]
    fn with_buffers<T>(&mut self, operation: impl FnOnce(&mut [u8], &mut [u8]) -> T) -> T {
        let mut input = [0; N];
        let mut checkpoint = [0; N];
        operation(&mut input, &mut checkpoint)
    }
}
