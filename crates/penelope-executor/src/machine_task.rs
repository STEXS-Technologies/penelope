//! Idempotent machine-to-task admission; execution stays in the native supervisor.
use async_trait::async_trait;
use penelope_core::{
    machine::MachineSourceId,
    task::{runtime::TaskPortError, *},
};
use penelope_ports::{machine::*, tasks::AsyncTaskJournal};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MachineTaskHandoffError {
    #[error(transparent)]
    Source(#[from] MachineStoreError),
    #[error(transparent)]
    Task(#[from] TaskPortError),
    #[error(transparent)]
    Binding(#[from] TaskError),
}
/// Trusted receiver policy plus authoritative native journal. Its identity binds
/// the host namespace, exact task definition and input schema. No method starts,
/// resumes, fences or automatically retries application execution.
pub struct MachineTaskReceiver<J: AsyncTaskJournal> {
    journal: Arc<J>,
    consumer: MachineSourceId,
    definition: TaskDefinition,
    input_schema: u32,
}
impl<J: AsyncTaskJournal> MachineTaskReceiver<J> {
    /// # Errors
    /// Refuses an invalid native task definition.
    pub fn new(
        journal: Arc<J>,
        namespace: MachineSourceId,
        definition: TaskDefinition,
        input_schema: u32,
    ) -> Result<Self, TaskError> {
        definition.validate()?;
        let mut identity = b"penelope.machine-task-receiver.v1\0".to_vec();
        identity.extend_from_slice(&namespace.bytes());
        identity.extend_from_slice(&definition.fingerprint().digest().0);
        identity.extend_from_slice(&input_schema.to_le_bytes());
        Ok(Self {
            journal,
            consumer: MachineSourceId::from_bytes(Digest::of(&identity).0),
            definition,
            input_schema,
        })
    }
    /// Structural binding for an already source-authorized handoff. This does not
    /// establish source existence, acceptance or runtime execution authority.
    /// # Errors
    /// Refuses wrong input identity, bytes or native definition limits.
    pub fn binding(
        &self,
        source: MachineIntentSource,
        payload: &[u8],
    ) -> Result<MachineTaskBinding, TaskError> {
        MachineTaskBinding::new(
            source,
            self.consumer,
            self.definition,
            payload,
            self.input_schema,
        )
    }
    /// Authenticates a live source lease before native admission. Reclaims only
    /// reconstruct the same intent; even an existing task that has started or
    /// faulted is never resubmitted/restarted here. A failed or canceled response
    /// can leave an accepted task: retry/reconcile with the same source identity.
    /// # Errors
    /// Refuses wrong receiver, unauthorized/stale source, history or native ACK.
    pub async fn accept<O: MachineIntentOutbox<Receipt = TaskRecord>>(
        &self,
        outbox: &O,
        context: &O::Context,
        lease: &MachineIntentLease,
        tick: TaskTick,
    ) -> Result<TaskRecord, MachineTaskHandoffError> {
        if outbox.consumer() != self.consumer {
            return Err(MachineStoreError::Conflict.into());
        }
        outbox.validate(context, lease).await?;
        let binding = self.binding(lease.metadata().source, lease.payload())?;
        if let Some(record) = self
            .journal
            .read(binding.scope(), TaskSequence::ZERO)
            .await?
        {
            self.verify_native(&binding, lease.payload(), &record)
                .await?;
            return Ok(record);
        }
        let proposed = binding.propose(tick)?;
        match self.journal.commit(&proposed, Some(lease.payload())).await {
            Ok(record) => {
                // Native acknowledgement is exact, including its original tick.
                if record != proposed {
                    return Err(TaskPortError::Corrupt.into());
                }
                self.verify_native(&binding, lease.payload(), &record)
                    .await?;
                Ok(record)
            }
            Err(TaskPortError::Conflict) => {
                // Another admitted copy may have chosen a different native tick.
                // Read and validate its exact immutable intent; never manufacture an ACK.
                let record = self
                    .journal
                    .read(binding.scope(), TaskSequence::ZERO)
                    .await?
                    .ok_or(TaskPortError::Conflict)?;
                self.verify_native(&binding, lease.payload(), &record)
                    .await?;
                Ok(record)
            }
            Err(error) => Err(error.into()),
        }
    }
    async fn verify_native(
        &self,
        binding: &MachineTaskBinding,
        payload: &[u8],
        receipt: &TaskRecord,
    ) -> Result<(), MachineTaskHandoffError> {
        binding.validate(receipt)?;
        let stored = self
            .journal
            .read(binding.scope(), TaskSequence::ZERO)
            .await?
            .ok_or(TaskPortError::Conflict)?;
        if stored != *receipt {
            return Err(TaskPortError::Corrupt.into());
        }
        let mut bytes = vec![
            0;
            usize::try_from(binding.input().bytes)
                .map_err(|_size| TaskPortError::Exhausted)?
        ];
        let len = self
            .journal
            .read_blob(binding.scope(), binding.input(), &mut bytes)
            .await?;
        if len != payload.len() || bytes != payload {
            return Err(TaskPortError::Corrupt.into());
        }
        Ok(())
    }
}
const fn verifier_error(error: MachineTaskHandoffError) -> MachineStoreError {
    match error {
        MachineTaskHandoffError::Source(error) => error,
        MachineTaskHandoffError::Task(TaskPortError::Unavailable | TaskPortError::Runtime) => {
            MachineStoreError::Unavailable
        }
        MachineTaskHandoffError::Task(TaskPortError::Exhausted) => MachineStoreError::Capacity,
        MachineTaskHandoffError::Task(TaskPortError::Conflict) => MachineStoreError::Conflict,
        MachineTaskHandoffError::Task(TaskPortError::Corrupt) => MachineStoreError::Corrupt,
        MachineTaskHandoffError::Binding(_) => MachineStoreError::Conflict,
    }
}
#[async_trait]
impl<J: AsyncTaskJournal> MachineIntentAcceptor for MachineTaskReceiver<J> {
    type Receipt = TaskRecord;
    fn identity(&self) -> MachineSourceId {
        self.consumer
    }
    async fn verify(
        &self,
        source: &MachineIntentSource,
        payload: &[u8],
        receipt: &TaskRecord,
    ) -> Result<MachineIntentAcceptance, MachineStoreError> {
        let binding = self
            .binding(*source, payload)
            .map_err(|_binding| MachineStoreError::Conflict)?;
        self.verify_native(&binding, payload, receipt)
            .await
            .map_err(verifier_error)?;
        Ok(MachineIntentAcceptance {
            source: *source,
            consumer: self.consumer,
            receipt_digest: binding
                .receipt_digest(receipt)
                .map_err(|_binding| MachineStoreError::Corrupt)?,
        })
    }
}
