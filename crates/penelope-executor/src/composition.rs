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

    /// Recovers an already accepted task after its outbox delivery was acknowledged.
    /// Reconstruct the committed action from authoritative saga events first.
    /// An empty journal cannot be admitted through this path: new intent still
    /// requires a leased delivery through `accept`.
    /// # Errors
    /// Rejects missing intent, substituted input/schema and unconfirmed fencing.
    pub fn recover<J: TaskJournal, R: TaskRuntime, const N: usize>(
        &self,
        journal: J,
        runtime: R,
        input: &[u8],
        schema: u32,
        tick: TaskTick,
    ) -> Result<TaskSupervisor<J, R, N>, TaskCompositionError> {
        self.binding
            .validate(self.binding.request(), self.binding.definition(), input)?;
        let supervisor = TaskSupervisor::restore(
            journal,
            runtime,
            self.binding.scope(),
            self.binding.definition(),
            tick,
        )?;
        if supervisor
            .projection()
            .input()
            .is_none_or(|reference| reference.schema != schema)
        {
            return Err(TaskCompositionError::BindingMismatch);
        }
        Ok(supervisor)
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

/// Stable typed result delivery reconstructed from a verified completed task.
/// The input identity excludes generation and transport leases; altered evidence
/// under the same identity conflicts instead of becoming a second mutation.
#[derive(Debug, Clone)]
pub struct TaskResultRelay {
    outcome: penelope_ports::tasks::VerifiedTaskEffectOutcome,
    input: penelope_domain::ProcessInput,
}

impl TaskResultRelay {
    /// Creates a replayable inbox input from already verified application output.
    /// Known failures become terminal failures; retry policy needs explicit
    /// application handling rather than an automatic second external mutation.
    /// # Errors
    /// Rejects unresolved evidence or an invalid derived input identity.
    pub fn new(
        outcome: penelope_ports::tasks::VerifiedTaskEffectOutcome,
    ) -> Result<Self, TaskCompositionError> {
        use penelope_core::task::Digest;
        use penelope_domain::{ContentDigest, ProcessInput, ProcessInputKind};
        use penelope_ports::ExternalEffectState;
        use std::fmt::Write as _;
        let identity = outcome.binding().relay_identity();
        let mut input_id = String::from("inp_task_");
        for byte in identity.0 {
            write!(input_id, "{byte:02x}")
                .map_err(|_format| TaskCompositionError::BindingMismatch)?;
        }
        let mut payload = b"penelope.task-effect-result.v1\0".to_vec();
        payload.extend_from_slice(&identity.0);
        payload.extend_from_slice(&outcome.generation().get().to_be_bytes());
        payload.extend_from_slice(&outcome.output().digest.0);
        payload.extend_from_slice(&outcome.output().bytes.to_be_bytes());
        payload.extend_from_slice(&outcome.output().schema.to_be_bytes());
        payload.extend_from_slice(&outcome.classifier().0);
        payload.push(match outcome.evidence().state {
            ExternalEffectState::Succeeded => 1,
            ExternalEffectState::KnownFailure => 2,
            ExternalEffectState::Unknown => return Err(TaskCompositionError::BindingMismatch),
        });
        match &outcome.evidence().external_reference_id {
            Some(reference) => {
                payload.push(1);
                payload.extend_from_slice(reference.as_str().as_bytes());
            }
            None => payload.push(0),
        }
        let action = &outcome.binding().request().action;
        let input = ProcessInput::new(
            action.tenant_id.clone(),
            action.process_id.clone(),
            input_id
                .try_into()
                .map_err(|_invalid| TaskCompositionError::BindingMismatch)?,
            ProcessInputKind::TaskEffectResult,
            ContentDigest(Digest::of(&payload).0),
        );
        Ok(Self { outcome, input })
    }

    #[must_use]
    pub const fn input(&self) -> &penelope_domain::ProcessInput {
        &self.input
    }

    /// Existing engine observation; no new saga action or event wire kinds.
    #[must_use]
    pub fn observation(&self) -> crate::engine::ActionResultObservation {
        use crate::engine::ActionResultObservation;
        use penelope_ports::ExternalEffectState;
        let action = self.outcome.binding().request().action.action_id.clone();
        match self.outcome.evidence().state {
            ExternalEffectState::Succeeded => ActionResultObservation::succeeded(action),
            ExternalEffectState::KnownFailure => ActionResultObservation::terminal_failure(action),
            ExternalEffectState::Unknown => ActionResultObservation::unknown(action),
        }
    }

    #[must_use]
    pub fn linear_input(
        &self,
        next_action_id: Option<penelope_domain::ActionId>,
    ) -> crate::engine::LinearSagaInput {
        crate::engine::LinearSagaInput::ActionResult {
            input_id: self.input.input_id.clone(),
            observation: self.observation(),
            next_action_id,
        }
    }

    #[must_use]
    pub fn graph_input(
        &self,
        next_action_id: Option<penelope_domain::ActionId>,
    ) -> crate::graph::GraphSagaInput {
        crate::graph::GraphSagaInput::ActionResult {
            input_id: self.input.input_id.clone(),
            observation: self.observation(),
            next_action_id,
        }
    }

    /// Commits the engine-produced result batch with the exact stable inbox input.
    /// The caller must validate the engine's required outcome plan first. This
    /// checks full binding scope and receipt, and never invents log sequences.
    /// Retrying the exact immutable batch recovers a lost acknowledgement.
    /// # Errors
    /// Rejects substituted inputs, scopes, source evidence and invalid receipts.
    pub async fn commit(
        &self,
        store: &impl ProcessStore,
        commit: &AtomicProcessCommit,
    ) -> Result<AtomicProcessCommitReceipt, TaskCompositionError> {
        if commit.input.as_ref() != Some(&self.input)
            || commit.canonical_source_event_id.is_some()
            || commit
                .outcomes
                .iter()
                .any(|outcome| outcome.scope() != self.outcome.binding().request().action.scope())
        {
            return Err(TaskCompositionError::BindingMismatch);
        }
        commit
            .validate()
            .map_err(|_invalid| TaskCompositionError::BindingMismatch)?;
        let receipt = store.commit(commit).await?;
        receipt
            .validate_for(commit)
            .map_err(|_invalid| TaskCompositionError::InvalidReceipt)?;
        Ok(receipt)
    }
}
