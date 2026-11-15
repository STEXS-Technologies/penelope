//! Exact machine-source/native-task binding; construction does not authenticate it.
use super::*;
use penelope_core::task::*;

impl MachineIntentSource {
    /// Stable full-source identity within a host-pinned receiver namespace. All
    /// coordinates are fixed-width and tagged separately from other protocols.
    /// This binds content; it does not authenticate a source or permit execution.
    pub fn binding_identity(self, consumer: MachineSourceId) -> [u8; 32] {
        let scope = self.head.scope;
        let mut bytes = b"penelope.machine-receiver-source.v1\0".to_vec();
        for value in [
            consumer.bytes(),
            scope.tenant.bytes(),
            scope.instance.bytes(),
            scope.definition.fingerprint.bytes(),
        ] {
            bytes.extend_from_slice(&value);
        }
        bytes.extend_from_slice(&scope.definition.state_schema.to_le_bytes());
        bytes.extend_from_slice(&scope.definition.event_schema.to_le_bytes());
        bytes.extend_from_slice(&self.head.revision.get().to_le_bytes());
        for value in [
            self.event.bytes(),
            self.commit.bytes(),
            self.intent.identity.bytes(),
            self.intent.digest,
        ] {
            bytes.extend_from_slice(&value);
        }
        bytes.extend_from_slice(&self.intent.payload_bytes.to_le_bytes());
        Digest::of(&bytes).0
    }
}
/// Immutable task identity derived from the complete source and receiver policy.
/// Task generations, ticks, history and execution authority remain native task
/// coordinates. Transport owner/token/expiry are deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineTaskBinding {
    source: MachineIntentSource,
    consumer: MachineSourceId,
    scope: TaskScope,
    definition: TaskDefinition,
    input: BlobRef,
}
impl MachineTaskBinding {
    /// # Errors
    /// Refuses invalid definition, empty source revision or mismatched/oversized bytes.
    pub fn new(
        source: MachineIntentSource,
        consumer: MachineSourceId,
        definition: TaskDefinition,
        payload: &[u8],
        schema: u32,
    ) -> Result<Self, TaskError> {
        definition.validate()?;
        let bytes = u32::try_from(payload.len()).map_err(|_size| TaskError::QuotaExhausted)?;
        let digest = Digest::of(payload);
        if bytes > definition.budget.max_input_bytes {
            return Err(TaskError::QuotaExhausted);
        }
        if source.head.revision == MachineRevision::ZERO
            || source.intent.payload_bytes != u64::from(bytes)
            || source.intent.digest != digest.0
        {
            return Err(TaskError::ScopeMismatch);
        }
        let mut tenant = b"penelope.machine-task-tenant.v1\0".to_vec();
        tenant.extend_from_slice(&source.head.scope.tenant.bytes());
        let scope = TaskScope {
            tenant: TaskTenantId::from_digest(Digest::of(&tenant)),
            task: TaskId::from_digest(Digest(source.binding_identity(consumer))),
            definition: definition.fingerprint(),
            input: TaskInputDigest::from_digest(digest),
        };
        Ok(Self {
            source,
            consumer,
            scope,
            definition,
            input: BlobRef {
                digest,
                bytes,
                schema,
            },
        })
    }
    pub const fn source(&self) -> MachineIntentSource {
        self.source
    }
    pub const fn consumer(&self) -> MachineSourceId {
        self.consumer
    }
    pub const fn scope(&self) -> TaskScope {
        self.scope
    }
    pub const fn definition(&self) -> TaskDefinition {
        self.definition
    }
    pub const fn input(&self) -> BlobRef {
        self.input
    }
    /// Proposes the existing native intent through the existing task validator.
    /// # Errors
    /// Refuses a native task policy/time/transition violation.
    pub fn propose(&self, tick: TaskTick) -> Result<TaskRecord, TaskError> {
        TaskProjection::new(self.scope, self.definition)?.propose(
            TaskExecutionEvent::IntentCommitted {
                definition: self.definition,
                input: self.input,
            },
            TaskGeneration::ZERO,
            tick,
        )
    }
    /// Checks the complete native accepted intent, without starting a supervisor.
    /// # Errors
    /// Refuses definition, input/schema, sequence, generation or record substitution.
    pub fn validate(&self, record: &TaskRecord) -> Result<(), TaskError> {
        let expected = self.propose(record.tick)?;
        if expected != *record {
            return Err(TaskError::ScopeMismatch);
        }
        Ok(())
    }
    /// Digest of the exact native receipt and full source binding, independent of
    /// transport leases. Requires authoritative native lookup by the receiver too.
    /// # Errors
    /// Refuses any receipt that fails native intent binding.
    pub fn receipt_digest(&self, record: &TaskRecord) -> Result<[u8; 32], TaskError> {
        self.validate(record)?;
        let mut bytes = b"penelope.machine-task-receipt.v1\0".to_vec();
        bytes.extend_from_slice(&self.source.binding_identity(self.consumer));
        // Native TaskRecord contains only fixed-size fields/enums, never caller strings/Vecs.
        serde_json::to_writer(&mut bytes, record).map_err(|_codec| TaskError::CorruptBlob)?;
        Ok(Digest::of(&bytes).0)
    }
}
