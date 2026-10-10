//! Deterministic, bounded task history validation and recovery decisions.
use super::{
    BlobRef, EffectContract, Reconciliation, RecoveryMode, TaskDefinition, TaskError,
    TaskExecutionEvent, TaskFault, TaskRecord, TaskScope, TaskStatus,
};

/// Fixed-size projection. Only validated records can change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskProjection {
    scope: TaskScope,
    definition: TaskDefinition,
    status: TaskStatus,
    next_sequence: u32,
    generation: u32,
    tick: u64,
    execution_deadline: u64,
    restart_at: u64,
    checkpoint_commits: u32,
    checkpoint: Option<BlobRef>,
    input: Option<BlobRef>,
    output: Option<BlobRef>,
}

impl TaskProjection {
    /// Creates an empty projection for an exact pinned definition.
    /// # Errors
    /// Returns definition validation failures.
    pub fn new(scope: TaskScope, definition: TaskDefinition) -> Result<Self, TaskError> {
        definition.validate()?;
        if scope.definition != definition.code {
            return Err(TaskError::ScopeMismatch);
        }
        Ok(Self {
            scope,
            definition,
            status: TaskStatus::Empty,
            next_sequence: 0,
            generation: 0,
            tick: 0,
            execution_deadline: 0,
            restart_at: 0,
            checkpoint_commits: 0,
            checkpoint: None,
            input: None,
            output: None,
        })
    }

    pub const fn status(&self) -> TaskStatus {
        self.status
    }
    pub const fn generation(&self) -> u32 {
        self.generation
    }
    pub const fn next_sequence(&self) -> u32 {
        self.next_sequence
    }
    pub const fn scope(&self) -> TaskScope {
        self.scope
    }
    pub const fn definition(&self) -> TaskDefinition {
        self.definition
    }
    pub const fn checkpoint(&self) -> Option<BlobRef> {
        self.checkpoint
    }
    pub const fn input(&self) -> Option<BlobRef> {
        self.input
    }
    pub const fn output(&self) -> Option<BlobRef> {
        self.output
    }
    pub const fn last_tick(&self) -> u64 {
        self.tick
    }
    pub const fn execution_deadline(&self) -> u64 {
        self.execution_deadline
    }

    /// Builds and validates a proposed record without mutating authoritative state.
    /// Persist atomically before applying it or issuing its runtime command.
    /// # Errors
    /// Returns the same validation failures as `apply`.
    pub fn propose(
        &self,
        event: TaskExecutionEvent,
        generation: u32,
        tick: u64,
    ) -> Result<TaskRecord, TaskError> {
        let record = TaskRecord {
            version: 1,
            scope: self.scope,
            sequence: self.next_sequence,
            generation,
            tick,
            event,
        };
        let mut candidate = *self;
        candidate.apply(&record)?;
        Ok(record)
    }

    /// Validates and applies one committed record, leaving state unchanged on error.
    /// # Errors
    /// Rejects invalid scope, order, generation, policy, quotas and transitions.
    pub fn apply(&mut self, record: &TaskRecord) -> Result<(), TaskError> {
        let mut candidate = *self;
        candidate.apply_inner(record)?;
        *self = candidate;
        Ok(())
    }

    fn apply_inner(&mut self, r: &TaskRecord) -> Result<(), TaskError> {
        if r.version != 1 {
            return Err(TaskError::UnsupportedVersion);
        }
        if r.scope != self.scope {
            return Err(TaskError::ScopeMismatch);
        }
        if r.sequence != self.next_sequence || r.tick < self.tick {
            return Err(TaskError::InvalidOrder);
        }
        if r.sequence >= self.definition.budget.max_history_records {
            return Err(TaskError::QuotaExhausted);
        }
        let starts = matches!(
            r.event,
            TaskExecutionEvent::Started | TaskExecutionEvent::Restarted
        );
        let expected = if starts {
            self.generation
                .checked_add(1)
                .ok_or(TaskError::QuotaExhausted)?
        } else {
            self.generation
        };
        if r.generation != expected {
            return Err(TaskError::StaleGeneration);
        }
        match r.event {
            TaskExecutionEvent::IntentCommitted { definition, input } => {
                self.require(TaskStatus::Empty)?;
                if definition != self.definition || input.digest != self.scope.input {
                    return Err(TaskError::ScopeMismatch);
                }
                if input.bytes > definition.budget.max_input_bytes || input.schema == 0 {
                    return Err(TaskError::QuotaExhausted);
                }
                self.input = Some(input);
                self.status = TaskStatus::Pending;
            }
            TaskExecutionEvent::CheckpointMigrated {
                source,
                previous,
                checkpoint,
                ..
            } => {
                self.require(TaskStatus::Pending)?;
                if self.checkpoint.is_some()
                    || source.tenant != self.scope.tenant
                    || source.task != self.scope.task
                    || source.definition == self.scope.definition
                    || previous.schema == 0
                {
                    return Err(TaskError::ScopeMismatch);
                }
                if checkpoint.schema != self.definition.checkpoint_schema {
                    return Err(TaskError::SchemaMismatch);
                }
                if checkpoint.bytes > self.definition.recovery.max_checkpoint_bytes
                    || self.checkpoint_commits >= self.definition.budget.max_checkpoint_commits
                {
                    return Err(TaskError::QuotaExhausted);
                }
                self.checkpoint_commits = self
                    .checkpoint_commits
                    .checked_add(1)
                    .ok_or(TaskError::QuotaExhausted)?;
                self.checkpoint = Some(checkpoint);
            }
            TaskExecutionEvent::Started | TaskExecutionEvent::Restarted => {
                let initial = matches!(r.event, TaskExecutionEvent::Started);
                self.require(if initial {
                    TaskStatus::Pending
                } else {
                    TaskStatus::Ready
                })?;
                if r.generation > self.definition.recovery.max_attempts {
                    return Err(TaskError::QuotaExhausted);
                }
                if r.tick >= self.definition.budget.deadline_tick {
                    return Err(TaskError::DeadlineExpired);
                }
                if !initial && r.tick < self.restart_at {
                    return Err(TaskError::InvalidTransition);
                }
                self.execution_deadline = r
                    .tick
                    .checked_add(self.definition.recovery.timeout_ticks)
                    .ok_or(TaskError::DeadlineExpired)?
                    .min(self.definition.budget.deadline_tick);
                self.generation = r.generation;
                self.status = TaskStatus::Running;
            }
            TaskExecutionEvent::CheckpointCommitted { checkpoint } => {
                self.require_live(r.tick)?;
                if checkpoint.schema != self.definition.checkpoint_schema {
                    return Err(TaskError::SchemaMismatch);
                }
                if checkpoint.bytes > self.definition.recovery.max_checkpoint_bytes
                    || self.checkpoint_commits >= self.definition.budget.max_checkpoint_commits
                {
                    return Err(TaskError::QuotaExhausted);
                }
                self.checkpoint_commits = self
                    .checkpoint_commits
                    .checked_add(1)
                    .ok_or(TaskError::QuotaExhausted)?;
                self.checkpoint = Some(checkpoint);
            }
            TaskExecutionEvent::FaultObserved { fault } => {
                self.require(TaskStatus::Running)?;
                if matches!(fault, TaskFault::Timeout) && r.tick < self.execution_deadline {
                    return Err(TaskError::InvalidTransition);
                }
                if matches!(fault, TaskFault::DeadlineViolation)
                    && r.tick < self.definition.budget.deadline_tick
                {
                    return Err(TaskError::InvalidTransition);
                }
                self.restart_at = r
                    .tick
                    .checked_add(self.definition.recovery.restart_delay_ticks)
                    .ok_or(TaskError::DeadlineExpired)?;
                self.status = TaskStatus::Faulted;
            }
            TaskExecutionEvent::ReconciliationRequired => {
                self.require(TaskStatus::Faulted)?;
                if !self.needs_reconciliation() {
                    return Err(TaskError::InvalidTransition);
                }
                self.status = TaskStatus::Reconciling;
            }
            TaskExecutionEvent::Reconciled { result } => {
                self.require(TaskStatus::Reconciling)?;
                match result {
                    Reconciliation::Completed { output, .. } => {
                        self.validate_output(output)?;
                        self.output = Some(output);
                        self.status = TaskStatus::Completed;
                    }
                    Reconciliation::RestartAuthorized { .. } => {
                        self.status = TaskStatus::Reconciled
                    }
                    Reconciliation::Unknown { .. } => {}
                }
            }
            TaskExecutionEvent::RecoveryPlanned { resume } => {
                self.require(if self.needs_reconciliation() {
                    TaskStatus::Reconciled
                } else {
                    TaskStatus::Faulted
                })?;
                if !self.can_recover(r.tick) {
                    return Err(TaskError::QuotaExhausted);
                }
                let expected_resume = match self.definition.recovery.mode {
                    RecoveryMode::RestartFromBeginning => None,
                    RecoveryMode::RestartFromCheckpoint | RecoveryMode::ReconcileBeforeRestart => {
                        self.checkpoint
                    }
                    RecoveryMode::NeverRestart => return Err(TaskError::InvalidTransition),
                };
                if resume != expected_resume {
                    return Err(TaskError::CorruptBlob);
                }
                self.checkpoint = resume;
                self.status = TaskStatus::Ready;
            }
            TaskExecutionEvent::Completed { output } => {
                self.require_live(r.tick)?;
                self.validate_output(output)?;
                self.output = Some(output);
                self.status = TaskStatus::Completed;
            }
            TaskExecutionEvent::RecoveryExhausted => {
                if !matches!(
                    self.status,
                    TaskStatus::Faulted
                        | TaskStatus::Reconciled
                        | TaskStatus::Pending
                        | TaskStatus::Ready
                        | TaskStatus::Reconciling
                ) {
                    return Err(TaskError::InvalidTransition);
                }
                if self.can_recover(r.tick)
                    && !matches!(self.status, TaskStatus::Pending | TaskStatus::Ready)
                {
                    return Err(TaskError::InvalidTransition);
                }
                if matches!(self.status, TaskStatus::Pending | TaskStatus::Ready)
                    && r.tick < self.definition.budget.deadline_tick
                {
                    return Err(TaskError::InvalidTransition);
                }
                self.status = TaskStatus::Exhausted;
            }
        }
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(TaskError::QuotaExhausted)?;
        self.tick = r.tick;
        Ok(())
    }

    const fn needs_reconciliation(&self) -> bool {
        matches!(self.definition.effects, EffectContract::ReconcileRequired)
            || matches!(
                self.definition.recovery.mode,
                RecoveryMode::ReconcileBeforeRestart
            )
    }

    const fn can_recover(&self, tick: u64) -> bool {
        !matches!(self.definition.recovery.mode, RecoveryMode::NeverRestart)
            && self.generation < self.definition.recovery.max_attempts
            && tick < self.definition.budget.deadline_tick
            && self.restart_at < self.definition.budget.deadline_tick
    }

    fn require(&self, status: TaskStatus) -> Result<(), TaskError> {
        if self.status != status {
            return Err(TaskError::InvalidTransition);
        }
        Ok(())
    }

    fn require_live(&self, tick: u64) -> Result<(), TaskError> {
        self.require(TaskStatus::Running)?;
        if tick >= self.execution_deadline {
            return Err(TaskError::DeadlineExpired);
        }
        Ok(())
    }

    const fn validate_output(&self, output: BlobRef) -> Result<(), TaskError> {
        if output.bytes > self.definition.budget.max_output_bytes || output.schema == 0 {
            return Err(TaskError::QuotaExhausted);
        }
        Ok(())
    }

    /// Chooses the next supervisory transition. Running tasks only time out here;
    /// restored running tasks must first be fenced and reported interrupted.
    /// # Errors
    /// Returns policy/order failures; `None` means wait for an observation.
    pub fn next(&self, tick: u64) -> Result<Option<TaskRecord>, TaskError> {
        if tick < self.tick {
            return Err(TaskError::InvalidOrder);
        }
        let event = match self.status {
            TaskStatus::Empty | TaskStatus::Completed | TaskStatus::Exhausted => return Ok(None),
            TaskStatus::Running => {
                if tick < self.execution_deadline {
                    return Ok(None);
                }
                TaskExecutionEvent::FaultObserved {
                    fault: TaskFault::Timeout,
                }
            }
            TaskStatus::Pending | TaskStatus::Ready
                if tick >= self.definition.budget.deadline_tick =>
            {
                TaskExecutionEvent::RecoveryExhausted
            }
            TaskStatus::Pending => TaskExecutionEvent::Started,
            TaskStatus::Ready => {
                if tick < self.restart_at {
                    return Ok(None);
                }
                TaskExecutionEvent::Restarted
            }
            TaskStatus::Faulted if self.needs_reconciliation() => {
                TaskExecutionEvent::ReconciliationRequired
            }
            TaskStatus::Faulted | TaskStatus::Reconciled => {
                if !self.can_recover(tick) {
                    TaskExecutionEvent::RecoveryExhausted
                } else {
                    TaskExecutionEvent::RecoveryPlanned {
                        resume: match self.definition.recovery.mode {
                            RecoveryMode::RestartFromBeginning | RecoveryMode::NeverRestart => None,
                            RecoveryMode::RestartFromCheckpoint
                            | RecoveryMode::ReconcileBeforeRestart => self.checkpoint,
                        },
                    }
                }
            }
            TaskStatus::Reconciling => return Ok(None),
        };
        let generation = if matches!(
            event,
            TaskExecutionEvent::Started | TaskExecutionEvent::Restarted
        ) {
            self.generation
                .checked_add(1)
                .ok_or(TaskError::QuotaExhausted)?
        } else {
            self.generation
        };
        self.propose(event, generation, tick).map(Some)
    }
}

/// Reconstructs a projection without re-executing task code or effects.
/// # Errors
/// Rejects malformed, oversized, foreign or incompatible histories.
pub fn replay(
    scope: TaskScope,
    definition: TaskDefinition,
    records: impl IntoIterator<Item = TaskRecord>,
) -> Result<TaskProjection, TaskError> {
    let mut projection = TaskProjection::new(scope, definition)?;
    for record in records {
        projection.apply(&record)?;
    }
    Ok(projection)
}
