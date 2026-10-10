//! Recoverable execution, separate from the immutable saga v1 protocol.
//!
//! All values have fixed size. Replay is bounded by the definition's record
//! quota and needs no allocation. Observations must come from a trusted
//! supervisor after it fences/stops the previous execution domain. A digest
//! proves byte identity, not application correctness or external effect success.

use sha2::{Digest as _, Sha256};
use thiserror::Error;

/// SHA-256 content identity; identities are scoped and supplied by the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Digest(pub [u8; 32]);

impl Digest {
    /// Computes the identity of an immutable blob.
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}

/// Recovery permission, never inferred from a generic failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RecoveryMode {
    RestartFromBeginning,
    RestartFromCheckpoint,
    ReconcileBeforeRestart,
    NeverRestart,
}

/// Whether interruption can leave ambiguous externally visible mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EffectContract {
    /// Computation only; the application attests it performs no external effects.
    Pure,
    /// Every interrupted generation must reconcile before recovery.
    ReconcileRequired,
}

/// Explicit recovery limits. Attempts include the initial generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct TaskRecoveryPolicy {
    pub mode: RecoveryMode,
    pub max_attempts: u32,
    pub timeout_ticks: u64,
    pub restart_delay_ticks: u64,
    pub max_checkpoint_bytes: u32,
}

/// Deployment quotas, measured by the trusted runtime/storage adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ExecutionBudget {
    pub max_input_bytes: u32,
    pub max_output_bytes: u32,
    pub max_history_records: u32,
    pub max_checkpoint_commits: u32,
    /// Per-generation CPU allowance; a kernel/runtime must enforce it.
    pub cpu_ticks: u64,
    /// Absolute monotonic deadline, independent of restarts.
    pub deadline_tick: u64,
}

/// Pinned task code/checkpoint schema and permission contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct TaskDefinition {
    pub code: Digest,
    pub checkpoint_schema: u32,
    pub effects: EffectContract,
    pub recovery: TaskRecoveryPolicy,
    pub budget: ExecutionBudget,
}

impl TaskDefinition {
    /// Stable protocol-v1 fingerprint of code, schema, effect permissions and
    /// every policy/budget field. Numeric values are encoded big-endian; enum
    /// tags are explicit and independent of Serde's representation.
    pub fn fingerprint(&self) -> Digest {
        let mut hash = Sha256::new();
        hash.update(b"penelope.task-definition.v1\0");
        hash.update(self.code.0);
        hash.update(self.checkpoint_schema.to_be_bytes());
        hash.update([match self.effects {
            EffectContract::Pure => 0,
            EffectContract::ReconcileRequired => 1,
        }]);
        hash.update([match self.recovery.mode {
            RecoveryMode::RestartFromBeginning => 0,
            RecoveryMode::RestartFromCheckpoint => 1,
            RecoveryMode::ReconcileBeforeRestart => 2,
            RecoveryMode::NeverRestart => 3,
        }]);
        hash.update(self.recovery.max_attempts.to_be_bytes());
        hash.update(self.recovery.timeout_ticks.to_be_bytes());
        hash.update(self.recovery.restart_delay_ticks.to_be_bytes());
        hash.update(self.recovery.max_checkpoint_bytes.to_be_bytes());
        hash.update(self.budget.max_input_bytes.to_be_bytes());
        hash.update(self.budget.max_output_bytes.to_be_bytes());
        hash.update(self.budget.max_history_records.to_be_bytes());
        hash.update(self.budget.max_checkpoint_commits.to_be_bytes());
        hash.update(self.budget.cpu_ticks.to_be_bytes());
        hash.update(self.budget.deadline_tick.to_be_bytes());
        Digest(hash.finalize().into())
    }

    /// Checks nonzero bounds and valid checkpoint schema.
    /// # Errors
    /// Returns `InvalidDefinition` for unusable limits.
    pub const fn validate(&self) -> Result<(), TaskError> {
        if self.recovery.max_attempts == 0
            || self.recovery.timeout_ticks == 0
            || self.budget.cpu_ticks == 0
            || self.budget.max_history_records < 4
            || self.budget.deadline_tick == 0
            || self.checkpoint_schema == 0
        {
            return Err(TaskError::InvalidDefinition);
        }
        Ok(())
    }
}

/// Stable execution identity. `task` is also the external effect idempotency key;
/// it must never change between generations. New definitions need new histories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct TaskScope {
    pub tenant: Digest,
    pub task: Digest,
    /// Fingerprint of the entire pinned definition, not just its code digest.
    pub definition: Digest,
    pub input: Digest,
}

/// Reference to bytes durably stored before the referencing history commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct BlobRef {
    pub digest: Digest,
    pub bytes: u32,
    pub schema: u32,
}

impl BlobRef {
    /// Checks identity, byte count and schema before supplying bytes to task code.
    /// # Errors
    /// Returns `CorruptBlob` or `SchemaMismatch`; no implicit fallback is allowed.
    pub fn verify(&self, bytes: &[u8], schema: u32) -> Result<(), TaskError> {
        if self.schema != schema {
            return Err(TaskError::SchemaMismatch);
        }
        if usize::try_from(self.bytes).ok() != Some(bytes.len()) || Digest::of(bytes) != self.digest
        {
            return Err(TaskError::CorruptBlob);
        }
        Ok(())
    }
}

/// Trusted runtime fault notification, never a fabricated completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TaskFault {
    Crash,
    ProtectionViolation,
    WatchdogExpired,
    Timeout,
    DeadlineViolation,
    CpuBudgetExceeded,
    SupervisorInterrupted,
    ApplicationFailure,
}

/// Independently obtained external effect evidence, scoped by the envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub enum Reconciliation {
    /// Authoritative output is durably available. Never derive from task exit alone.
    Completed { output: BlobRef, evidence: Digest },
    /// Trusted adapter proves that retry is permitted under the application's contract.
    RestartAuthorized { evidence: Digest },
    /// Remains ambiguous; no restart or completion may follow this observation.
    Unknown { evidence: Digest },
}

/// Task history protocol v1. This does not extend any saga protocol enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub enum TaskExecutionEvent {
    IntentCommitted {
        definition: TaskDefinition,
        input: BlobRef,
    },
    /// Explicit application-authorized state transformation into a new history.
    /// Source generation must already be fenced by the consuming application.
    CheckpointMigrated {
        source: TaskScope,
        previous: BlobRef,
        checkpoint: BlobRef,
        authorization: Digest,
    },
    Started,
    CheckpointCommitted {
        checkpoint: BlobRef,
    },
    FaultObserved {
        fault: TaskFault,
    },
    ReconciliationRequired,
    Reconciled {
        result: Reconciliation,
    },
    RecoveryPlanned {
        resume: Option<BlobRef>,
    },
    Restarted,
    Completed {
        output: BlobRef,
    },
    RecoveryExhausted,
}

/// Ordered, independently versioned task record. Sequence begins at zero;
/// initial intent has generation zero, first execution has generation one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct TaskRecord {
    pub version: u16,
    pub scope: TaskScope,
    pub sequence: u32,
    pub generation: u32,
    pub tick: u64,
    pub event: TaskExecutionEvent,
}

/// Recoverable execution projection phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Empty,
    Pending,
    Running,
    Faulted,
    Reconciling,
    Reconciled,
    Ready,
    Completed,
    Exhausted,
}

/// Typed fail-closed decision errors.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum TaskError {
    #[error("invalid task definition")]
    InvalidDefinition,
    #[error("task history version is unsupported")]
    UnsupportedVersion,
    #[error("task scope or pinned definition does not match")]
    ScopeMismatch,
    #[error("history sequence or monotonic time is invalid")]
    InvalidOrder,
    #[error("stale task generation")]
    StaleGeneration,
    #[error("event is not permitted in current task phase")]
    InvalidTransition,
    #[error("execution or recovery quota exhausted")]
    QuotaExhausted,
    #[error("checkpoint or input/output bytes are corrupt")]
    CorruptBlob,
    #[error("checkpoint schema needs explicit application migration")]
    SchemaMismatch,
    #[error("execution deadline expired")]
    DeadlineExpired,
}

/// Cooperative application computation. Implementations own their correctness
/// and safety; this interface does not preempt code or isolate memory.
pub trait RecoverableTask {
    type Input;
    type Checkpoint;
    type Output;
    type Error;
    fn execute(
        &mut self,
        input: &Self::Input,
        checkpoint: Option<&Self::Checkpoint>,
    ) -> TaskProgress<Self::Checkpoint, Self::Output, Self::Error>;
}

/// Application proposals; checkpoints/results become facts only after commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskProgress<C, O, E> {
    Checkpoint(C),
    Complete(O),
    Failed(E),
}
