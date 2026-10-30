//! Recoverable joins between independently durable saga and task logs.
//! An exact ambiguous process commit must be retried or reconstructed before
//! dispatch. No cross-store transaction is assumed.
use crate::task::{SupervisorError, TaskSupervisor};
use penelope_core::task::{
    TaskError, TaskStatus, TaskTick,
    runtime::{TaskJournal, TaskRuntime},
};
use penelope_domain::LogicalTime;
use penelope_ports::{
    AtomicProcessCommit, AtomicProcessCommitReceipt, OutboxClaimRequest, OutboxLease, OutboxStore,
    PortError, ProcessStore, tasks::TaskEffectBinding,
};

#[derive(Debug, thiserror::Error)]
pub enum TaskCompositionError {
    #[error("dispatch does not match the committed action or lease")]
    BindingMismatch,
    #[error("process store returned an invalid commit acknowledgement")]
    InvalidReceipt,
    #[error(transparent)]
    Process(#[from] PortError),
    #[error(transparent)]
    Task(#[from] TaskError),
    #[error(transparent)]
    Supervisor(#[from] SupervisorError),
}

/// Dispatch authorization obtained only after validating a process-store receipt.
/// Repeating the exact commit reconstructs this value after a lost receipt.
/// The injected store must enforce immutable commits and outbox atomicity.
#[derive(Debug, Clone)]
pub struct CommittedTaskEffect {
    binding: TaskEffectBinding,
    receipt: AtomicProcessCommitReceipt,
}

/// Commits a normal engine-produced batch, then authorizes its exact task action.
/// This function does not generate outcomes or bypass engine decision validation.
/// # Errors
/// Rejects missing/substituted actions and invalid or ambiguous store receipts.
pub async fn commit_task_effect(
    store: &impl ProcessStore,
    commit: &AtomicProcessCommit,
    binding: TaskEffectBinding,
) -> Result<CommittedTaskEffect, TaskCompositionError> {
    commit
        .validate()
        .map_err(|_invalid| TaskCompositionError::BindingMismatch)?;
    if !commit
        .actions
        .iter()
        .any(|action| action == &binding.request().action)
    {
        return Err(TaskCompositionError::BindingMismatch);
    }
    let receipt = store.commit(commit).await?;
    receipt
        .validate_for(commit)
        .map_err(|_invalid| TaskCompositionError::InvalidReceipt)?;
    Ok(CommittedTaskEffect { binding, receipt })
}

impl CommittedTaskEffect {
    /// Validated acknowledgement of the exact batch that persisted this action.
    #[must_use]
    pub const fn receipt(&self) -> AtomicProcessCommitReceipt {
        self.receipt
    }

    #[must_use]
    pub const fn binding(&self) -> &TaskEffectBinding {
        &self.binding
    }

    /// Reconstructs or commits the exact task intent, then acknowledges its
    /// outbox lease only after durable downstream acceptance. It never starts
    /// worker code. The caller drives the returned supervisor. A failed ACK may
    /// leave a committed task; replay with the same binding and immutable input.
    ///
    /// Existing interrupted workers are fenced by normal supervisor recovery.
    /// Schema substitution is rejected even when the input digest matches.
    /// # Errors
    /// Any lease, binding, history, fence, commit or acknowledgement failure
    /// prevents returning dispatch authority. Reconstruct after ambiguity.
    #[allow(clippy::too_many_arguments)]
    pub async fn accept<J: TaskJournal, R: TaskRuntime, const N: usize>(
        &self,
        outbox: &impl OutboxStore,
        claim: &OutboxClaimRequest,
        lease: &OutboxLease,
        now: LogicalTime,
        journal: J,
        runtime: R,
        input: &[u8],
        schema: u32,
        tick: TaskTick,
    ) -> Result<TaskSupervisor<J, R, N>, TaskCompositionError> {
        lease.validate_for_claim_at(claim, now)?;
        if lease.record.action != self.binding.request().action {
            return Err(TaskCompositionError::BindingMismatch);
        }
        self.binding
            .validate(self.binding.request(), self.binding.definition(), input)?;
        let mut supervisor = TaskSupervisor::restore(
            journal,
            runtime,
            self.binding.scope(),
            self.binding.definition(),
            tick,
        )?;
        if supervisor.projection().status() == TaskStatus::Empty {
            supervisor.submit(input, schema, tick)?;
        } else if supervisor
            .projection()
            .input()
            .is_none_or(|reference| reference.schema != schema)
        {
            return Err(TaskCompositionError::BindingMismatch);
        }
        outbox.acknowledge(lease, now).await?;
        Ok(supervisor)
    }
}
