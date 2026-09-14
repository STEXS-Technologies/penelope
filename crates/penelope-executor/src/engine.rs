//! Deterministic linear saga planning with typed inputs and outputs.

use penelope_domain::{
    ActionId, ContentDigest, DefinitionId, DefinitionVersion, LogicalTimeV1, MAX_DEFINITION_STEPS,
    ProcessActionDtoV1, ProcessActionKindV1, ProcessId, ProcessScopeV1, StepId, TenantId,
};
use penelope_ports::{ManualReviewResolutionV1, TimerScheduleV1};
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU32, NonZeroU64};
use thiserror::Error;

/// A declared process step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepPlanV1 {
    /// Stable step identity from the pinned definition.
    pub step_id: StepId,
    /// The action category dispatched for this step.
    pub action_kind: ProcessActionKindV1,
    /// Digest of immutable action parameters.
    pub payload_digest: ContentDigest,
    /// Bounded retry behavior for this step.
    pub retry_policy: RetryPolicyV1,
    /// Optional compensating action to run after a known terminal forward failure.
    pub compensation: Option<CompensationPlanV1>,
}

/// Declared action used to compensate a previously succeeded step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationPlanV1 {
    /// Action category for the compensating effect.
    pub action_kind: ProcessActionKindV1,
    /// Digest of immutable compensation parameters.
    pub payload_digest: ContentDigest,
    /// Bounded retry behavior for the compensating action.
    pub retry_policy: RetryPolicyV1,
}

impl CompensationPlanV1 {
    /// Declares a canonical command compensation action.
    pub const fn canonical_command(
        payload_digest: ContentDigest,
        retry_policy: RetryPolicyV1,
    ) -> Self {
        Self {
            action_kind: ProcessActionKindV1::CanonicalCommand,
            payload_digest,
            retry_policy,
        }
    }
}

/// Explicit retry bound for one action step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicyV1 {
    /// Total number of permitted attempts, including the first attempt.
    pub max_attempts: NonZeroU32,
    /// Optional deterministic exponential retry timing policy.
    pub backoff: Option<RetryBackoffV1>,
}

/// Bounded deterministic exponential retry timing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryBackoffV1 {
    /// Delay for the first retry in logical milliseconds.
    pub base_delay_millis: NonZeroU64,
    /// Maximum delay after exponential growth in logical milliseconds.
    pub max_delay_millis: NonZeroU64,
}

impl RetryBackoffV1 {
    /// Creates a bounded exponential retry timing policy.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidRetryBackoff`] when the first delay exceeds
    /// the configured maximum delay.
    pub const fn new(
        base_delay_millis: NonZeroU64,
        max_delay_millis: NonZeroU64,
    ) -> Result<Self, EngineError> {
        if base_delay_millis.get() > max_delay_millis.get() {
            return Err(EngineError::InvalidRetryBackoff);
        }
        Ok(Self {
            base_delay_millis,
            max_delay_millis,
        })
    }
}

impl RetryPolicyV1 {
    /// Creates a retry policy with the given total attempt bound.
    pub const fn new(max_attempts: NonZeroU32) -> Self {
        Self {
            max_attempts,
            backoff: None,
        }
    }

    /// Creates a policy permitting exactly one attempt and no retry.
    pub const fn no_retry() -> Self {
        Self {
            max_attempts: NonZeroU32::MIN,
            backoff: None,
        }
    }

    /// Attaches deterministic retry timing to a bounded attempt policy.
    pub const fn with_backoff(mut self, backoff: RetryBackoffV1) -> Self {
        self.backoff = Some(backoff);
        self
    }

    /// Calculates the due time for a zero-based retry ordinal.
    ///
    /// The first retry uses `base_delay_millis`; each later retry doubles the
    /// delay and caps it at `max_delay_millis`. No wall clock is read here.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::RetryDueTimeOverflow`] when the supplied logical
    /// time cannot represent the calculated due time.
    pub fn retry_due_at(
        &self,
        now: LogicalTimeV1,
        retry_ordinal: u32,
    ) -> Result<Option<LogicalTimeV1>, EngineError> {
        let Some(backoff) = self.backoff else {
            return Ok(None);
        };
        let uncapped_delay = backoff
            .base_delay_millis
            .get()
            .checked_shl(retry_ordinal)
            .unwrap_or(u64::MAX);
        let delay = uncapped_delay.min(backoff.max_delay_millis.get());
        let due_at = now
            .0
            .checked_add(delay)
            .ok_or(EngineError::RetryDueTimeOverflow)?;
        Ok(Some(LogicalTimeV1(due_at)))
    }
}

impl StepPlanV1 {
    /// Declares a canonical-command step with a typed retry bound.
    pub const fn canonical_command(
        step_id: StepId,
        payload_digest: ContentDigest,
        retry_policy: RetryPolicyV1,
    ) -> Self {
        Self {
            step_id,
            action_kind: ProcessActionKindV1::CanonicalCommand,
            payload_digest,
            retry_policy,
            compensation: None,
        }
    }

    /// Attaches the compensating action declared for this forward step.
    pub const fn with_compensation(mut self, compensation: CompensationPlanV1) -> Self {
        self.compensation = Some(compensation);
        self
    }
}

/// A deterministic, ordered saga definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearSagaDefinitionV1 {
    /// Stable immutable definition identity.
    pub definition_id: DefinitionId,
    /// Pinned definition version selected when this process starts.
    pub definition_version: DefinitionVersion,
    /// Digest of the exact definition semantics used for replay.
    pub definition_digest: ContentDigest,
    /// Steps execute in vector order.
    pub steps: Vec<StepPlanV1>,
}

impl LinearSagaDefinitionV1 {
    /// Creates an ordered linear-saga definition.
    pub const fn new(
        definition_id: DefinitionId,
        definition_version: DefinitionVersion,
        definition_digest: ContentDigest,
        steps: Vec<StepPlanV1>,
    ) -> Self {
        Self {
            definition_id,
            definition_version,
            definition_digest,
            steps,
        }
    }
}

/// A replayable process projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearSagaProjectionV1 {
    /// Pinned immutable definition identity for this process instance.
    pub definition_id: DefinitionId,
    /// Pinned immutable definition version for this process instance.
    pub definition_version: DefinitionVersion,
    /// Pinned exact definition semantics digest for this process instance.
    pub definition_digest: ContentDigest,
    /// Index of the current step.
    pub next_step_index: usize,
    /// Zero-based attempt for the current step.
    pub current_attempt: u32,
    /// Due time while a retry timer is pending; no effect may run before it fires.
    pub retry_due_at: Option<LogicalTimeV1>,
    /// Stable identity of the action whose result may advance this projection.
    pub active_action_id: Option<ActionId>,
    /// Every action identity issued by this process, in durable decision order.
    pub issued_action_ids: Vec<ActionId>,
    /// Succeeded forward steps with declared compensation, in success order.
    pub compensable_step_indices: Vec<usize>,
    /// Terminal/non-terminal process status.
    pub status: SagaStatusV1,
}

/// Process lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SagaStatusV1 {
    /// A step is pending or executing.
    Running,
    /// Every declared step succeeded.
    Completed,
    /// A terminal result requires human intervention.
    Escalated,
    /// An authorized manual-review resolution cancelled the process.
    Cancelled,
    /// Every required compensation action completed in reverse success order.
    Compensated,
    /// A compensating action is currently pending or executing.
    Compensating,
}

/// Observed terminal action result supplied as data to the pure engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionResultV1 {
    /// The current action completed successfully.
    Succeeded,
    /// The action failed and may be retried with a new idempotent attempt.
    RetryableFailure,
    /// The action failed permanently and requires escalation.
    TerminalFailure,
    /// External outcome is ambiguous and must be reconciled before retry.
    Unknown,
}

/// An action result correlated to the independently idempotent action that
/// produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionResultObservationV1 {
    /// The action identity reported by the external effect boundary.
    pub action_id: ActionId,
    /// The classified external result.
    pub result: ActionResultV1,
}

/// Typed input accepted by the pure linear-saga decision function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinearSagaInputV1 {
    /// Start a previously absent process with its first action identity.
    Start {
        /// Fresh independently idempotent first action identity.
        action_id: ActionId,
    },
    /// Apply a durably observed action result to an existing projection.
    ActionResult {
        /// Correlated immutable effect result.
        observation: ActionResultObservationV1,
        /// Already-persisted fresh identity for a following action or retry.
        next_action_id: Option<ActionId>,
    },
    /// Record a retryable effect failure and durably plan its timer action.
    RetryTimerScheduled {
        /// Correlated retryable result for the active effect action.
        observation: ActionResultObservationV1,
        /// Fresh timer action identity, or a compensation action if retries are exhausted.
        next_action_id: Option<ActionId>,
        /// Logical time at which the retryable result was durably observed.
        observed_at: LogicalTimeV1,
    },
    /// Deliver one due retry timer firing and plan its next effect action.
    RetryTimerFired {
        /// Active timer action identity.
        timer_action_id: ActionId,
        /// Logical time at which the timer was delivered.
        fired_at: LogicalTimeV1,
        /// Fresh independently idempotent identity for the retry effect action.
        next_action_id: ActionId,
    },
    /// Apply an already-authorized immutable manual-review resolution.
    ManualResolution {
        /// Typed resolution recorded by the manual-review port.
        resolution: ManualReviewResolutionV1,
        /// Fresh action identity when the resolution resumes work.
        next_action_id: Option<ActionId>,
    },
}

/// One immutable event from the linear engine's ordered process log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinearSagaEventV1 {
    /// The durable process-start record and its first planned action identity.
    Started {
        /// Immutable definition identity selected when this process began.
        definition_id: DefinitionId,
        /// Immutable definition version selected when this process began.
        definition_version: DefinitionVersion,
        /// Exact definition semantics selected when this process began.
        definition_digest: ContentDigest,
        /// Identity for the first independently idempotent action.
        action_id: ActionId,
    },
    /// A recorded, correlated action result and the identity already planned
    /// for the next action, if that transition needs one.
    ActionResultObserved {
        /// Result accepted from the effect/canonical evidence boundary.
        observation: ActionResultObservationV1,
        /// Persisted identity for the following action or retry.
        next_action_id: Option<ActionId>,
    },
    /// A retryable effect result was recorded with a durable timer decision.
    RetryTimerScheduled {
        /// Correlated retryable result for the active effect action.
        observation: ActionResultObservationV1,
        /// Persisted timer action identity, or a compensation action if exhausted.
        next_action_id: Option<ActionId>,
        /// Logical time at which the retryable result was durably observed.
        observed_at: LogicalTimeV1,
    },
    /// A due retry timer fired and unlocked its retry effect action.
    RetryTimerFired {
        /// Active timer action identity.
        timer_action_id: ActionId,
        /// Logical time at which the timer was delivered.
        fired_at: LogicalTimeV1,
        /// Persisted fresh identity for the retry effect action.
        next_action_id: ActionId,
    },
    /// An authorized immutable manual-review resolution was accepted.
    ManualResolutionApplied {
        /// Typed operator resolution.
        resolution: ManualReviewResolutionV1,
        /// Persisted fresh action identity when the resolution resumes work.
        next_action_id: Option<ActionId>,
    },
}

/// One sequenced immutable event from a process outcome log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinearSagaEventEnvelopeV1 {
    /// Zero-based, contiguous event sequence for one process.
    pub sequence: u64,
    /// The event recorded at `sequence`.
    pub event: LinearSagaEventV1,
}

impl ActionResultObservationV1 {
    /// Records a successful result for `action_id`.
    pub const fn succeeded(action_id: ActionId) -> Self {
        Self {
            action_id,
            result: ActionResultV1::Succeeded,
        }
    }

    /// Records a retryable failure for `action_id`.
    pub const fn retryable_failure(action_id: ActionId) -> Self {
        Self {
            action_id,
            result: ActionResultV1::RetryableFailure,
        }
    }

    /// Records an ambiguous result for `action_id`.
    pub const fn unknown(action_id: ActionId) -> Self {
        Self {
            action_id,
            result: ActionResultV1::Unknown,
        }
    }

    /// Records a known terminal failure for `action_id`.
    pub const fn terminal_failure(action_id: ActionId) -> Self {
        Self {
            action_id,
            result: ActionResultV1::TerminalFailure,
        }
    }
}

impl LinearSagaEventV1 {
    /// Records a start event pinned to the exact definition used by the process.
    pub fn started(definition: &LinearSagaDefinitionV1, action_id: ActionId) -> Self {
        Self::Started {
            definition_id: definition.definition_id.clone(),
            definition_version: definition.definition_version.clone(),
            definition_digest: definition.definition_digest,
            action_id,
        }
    }
}

/// A deterministic decision from a transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SagaDecisionV1 {
    /// Resulting replayable projection.
    pub projection: LinearSagaProjectionV1,
    /// At most one next action for this linear reference engine.
    pub next_action: Option<ProcessActionDtoV1>,
}

impl SagaDecisionV1 {
    /// Returns the durable timer schedule required for a pending retry, if any.
    ///
    /// The outer application must atomically persist this decision before it
    /// asks a scheduler to deliver the timer action. A decision with a timer
    /// must not be sent to the ordinary action dispatcher.
    pub fn retry_timer_schedule(&self) -> Option<TimerScheduleV1> {
        Some(TimerScheduleV1 {
            action: self.next_action.clone()?,
            due_at: self.projection.retry_due_at?,
        })
    }
}

/// Engine invariant failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// A retry backoff's first delay exceeds its configured maximum delay.
    #[error("retry backoff base delay exceeds its maximum delay")]
    InvalidRetryBackoff,
    /// A calculated retry due time exceeds the logical time range.
    #[error("retry due time overflowed")]
    RetryDueTimeOverflow,
    /// A definition has no executable steps.
    #[error("saga definition has no steps")]
    EmptyDefinition,
    /// A definition exceeds the bounded executable step limit.
    #[error("saga definition exceeds the step limit")]
    DefinitionStepLimitExceeded,
    /// A definition declares the same step identity more than once.
    #[error("saga definition contains a duplicate step identifier")]
    DuplicateStepId,
    /// A projection points outside the pinned definition.
    #[error("projection step index is outside the definition")]
    InvalidProjection,
    /// A transition was requested after a terminal state.
    #[error("saga is already terminal")]
    TerminalProjection,
    /// Retrying the action would overflow its bounded attempt number.
    #[error("action attempt number overflowed")]
    AttemptOverflow,
    /// The observed result does not belong to the projection's active action.
    #[error("observed result does not match the active action")]
    UnexpectedAction,
    /// A non-terminal transition did not receive an ID for its next action.
    #[error("next action identity is required")]
    MissingNextAction,
    /// A transition attempted to reuse an action identity from this process.
    #[error("action identity was already issued by this process")]
    ReusedActionId,
    /// An ordered log had no durable start record.
    #[error("saga log has no start event")]
    MissingStart,
    /// An ordered log attempted to start an existing process again.
    #[error("saga log contains more than one start event")]
    DuplicateStart,
    /// An event log is not zero-based and contiguous.
    #[error("saga event sequence is not contiguous")]
    InvalidEventSequence,
    /// The event sequence cannot be advanced without overflow.
    #[error("saga event sequence overflowed")]
    EventSequenceOverflow,
    /// The supplied definition does not match the process's pinned definition.
    #[error("supplied definition does not match the pinned process definition")]
    DefinitionMismatch,
    /// A result input requires a projection rebuilt from durable outcomes.
    #[error("saga result input requires an existing projection")]
    MissingProjection,
    /// A start input may not replace an existing projection.
    #[error("saga start input requires an absent projection")]
    StartWithProjection,
    /// A backoff-enabled retry must be recorded through a timer decision.
    #[error("retry with backoff requires a durable timer decision")]
    RetryTimerRequired,
    /// A retry timer was requested for a policy without backoff timing.
    #[error("retry timer was requested without a retry backoff policy")]
    RetryTimerNotConfigured,
    /// A retry timer was requested although the retry budget is exhausted.
    #[error("retry timer is not needed because the retry budget is exhausted")]
    RetryTimerNotRequired,
    /// A non-retryable result attempted to schedule a retry timer.
    #[error("retry timer requires a retryable failure result")]
    RetryTimerRequiresRetryableFailure,
    /// A normal action-result transition attempted to consume a pending timer.
    #[error("pending retry timer must be delivered through a timer firing input")]
    RetryTimerMustFire,
    /// A timer firing did not match the pending timer action.
    #[error("retry timer firing does not match the pending timer action")]
    UnexpectedRetryTimer,
    /// A timer firing was delivered before its durable due time.
    #[error("retry timer fired before its durable due time")]
    RetryTimerFiredEarly,
    /// A manual resolution was supplied for a process that is not escalated.
    #[error("manual resolution requires an escalated process")]
    ManualResolutionRequiresEscalation,
}

impl LinearSagaDefinitionV1 {
    /// Validates the definition's minimum executable invariant.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::EmptyDefinition`] when no step is declared.
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.steps.is_empty() {
            return Err(EngineError::EmptyDefinition);
        }
        if self.steps.len() > MAX_DEFINITION_STEPS {
            return Err(EngineError::DefinitionStepLimitExceeded);
        }
        if self.steps.iter().enumerate().any(|(index, step)| {
            self.steps
                .iter()
                .skip(index.saturating_add(1))
                .any(|other_step| other_step.step_id == step.step_id)
        }) {
            return Err(EngineError::DuplicateStepId);
        }
        Ok(())
    }
}

/// Makes one deterministic, side-effect-free linear-saga decision.
///
/// The outer application must durably record the accepted input and resulting
/// decision before dispatching `next_action`. This function neither reads time
/// nor performs I/O.
///
/// # Errors
///
/// Returns a typed error when the input/projection combination is illegal or
/// the delegated start/result transition violates an engine invariant.
pub fn decide(
    definition: &LinearSagaDefinitionV1,
    projection: Option<&LinearSagaProjectionV1>,
    tenant_id: TenantId,
    process_id: ProcessId,
    input: &LinearSagaInputV1,
) -> Result<SagaDecisionV1, EngineError> {
    match input {
        LinearSagaInputV1::Start { action_id } => {
            if projection.is_some() {
                return Err(EngineError::StartWithProjection);
            }
            start(definition, tenant_id, process_id, action_id.clone())
        }
        LinearSagaInputV1::ActionResult {
            observation,
            next_action_id,
        } => apply_action_result(
            definition,
            projection.ok_or(EngineError::MissingProjection)?,
            tenant_id,
            process_id,
            observation,
            next_action_id.clone(),
        ),
        LinearSagaInputV1::RetryTimerScheduled {
            observation,
            next_action_id,
            observed_at,
        } => schedule_retry_timer(
            definition,
            projection.ok_or(EngineError::MissingProjection)?,
            tenant_id,
            process_id,
            observation,
            next_action_id.clone(),
            *observed_at,
        ),
        LinearSagaInputV1::RetryTimerFired {
            timer_action_id,
            fired_at,
            next_action_id,
        } => fire_retry_timer(
            definition,
            projection.ok_or(EngineError::MissingProjection)?,
            tenant_id,
            process_id,
            timer_action_id,
            *fired_at,
            next_action_id.clone(),
        ),
        LinearSagaInputV1::ManualResolution {
            resolution,
            next_action_id,
        } => apply_manual_resolution(
            definition,
            projection.ok_or(EngineError::MissingProjection)?,
            tenant_id,
            process_id,
            *resolution,
            next_action_id.clone(),
        ),
    }
}

/// Plans the first durable action for a newly created process.
///
/// # Errors
///
/// Returns [`EngineError::EmptyDefinition`] when the definition has no steps.
pub fn start(
    definition: &LinearSagaDefinitionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    action_id: ActionId,
) -> Result<SagaDecisionV1, EngineError> {
    definition.validate()?;
    let projection = LinearSagaProjectionV1 {
        definition_id: definition.definition_id.clone(),
        definition_version: definition.definition_version.clone(),
        definition_digest: definition.definition_digest,
        next_step_index: 0,
        current_attempt: 0,
        retry_due_at: None,
        active_action_id: Some(action_id.clone()),
        issued_action_ids: vec![action_id],
        compensable_step_indices: Vec::new(),
        status: SagaStatusV1::Running,
    };
    Ok(SagaDecisionV1 {
        next_action: Some(action_for(definition, &projection, tenant_id, process_id)?),
        projection,
    })
}

/// Applies one observed action result and plans the next action when safe.
///
/// An unknown result deliberately escalates: retries require reconciliation
/// evidence and cannot be inferred by this pure function.
///
/// # Errors
///
/// Returns an invariant error for invalid/terminal projections.
pub fn apply_action_result(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    observation: &ActionResultObservationV1,
    next_action_id: Option<ActionId>,
) -> Result<SagaDecisionV1, EngineError> {
    definition.validate()?;
    if projection.definition_id != definition.definition_id
        || projection.definition_version != definition.definition_version
        || projection.definition_digest != definition.definition_digest
    {
        return Err(EngineError::DefinitionMismatch);
    }
    if projection.status != SagaStatusV1::Running && projection.status != SagaStatusV1::Compensating
    {
        return Err(EngineError::TerminalProjection);
    }
    if projection.next_step_index >= definition.steps.len() {
        return Err(EngineError::InvalidProjection);
    }
    if projection.active_action_id.as_ref() != Some(&observation.action_id) {
        return Err(EngineError::UnexpectedAction);
    }
    if projection.retry_due_at.is_some() {
        return Err(EngineError::RetryTimerMustFire);
    }
    match projection.status {
        SagaStatusV1::Running => apply_forward_result(
            definition,
            projection,
            tenant_id,
            process_id,
            observation.result,
            next_action_id,
        ),
        SagaStatusV1::Compensating => apply_compensation_result(
            definition,
            projection,
            tenant_id,
            process_id,
            observation.result,
            next_action_id,
        ),
        SagaStatusV1::Completed
        | SagaStatusV1::Escalated
        | SagaStatusV1::Cancelled
        | SagaStatusV1::Compensated => Err(EngineError::TerminalProjection),
    }
}

/// Records a retryable result and plans a durable timer instead of dispatching
/// the retry effect immediately.
///
/// # Errors
///
/// Returns a typed invariant error when the observation is not the active
/// retryable effect, its policy lacks backoff timing, or the projection cannot
/// safely plan the timer action.
pub fn schedule_retry_timer(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    observation: &ActionResultObservationV1,
    next_action_id: Option<ActionId>,
    observed_at: LogicalTimeV1,
) -> Result<SagaDecisionV1, EngineError> {
    definition.validate()?;
    if projection.definition_id != definition.definition_id
        || projection.definition_version != definition.definition_version
        || projection.definition_digest != definition.definition_digest
    {
        return Err(EngineError::DefinitionMismatch);
    }
    if projection.status != SagaStatusV1::Running {
        return Err(EngineError::TerminalProjection);
    }
    if projection.next_step_index >= definition.steps.len() {
        return Err(EngineError::InvalidProjection);
    }
    if projection.active_action_id.as_ref() != Some(&observation.action_id) {
        return Err(EngineError::UnexpectedAction);
    }
    if projection.retry_due_at.is_some() {
        return Err(EngineError::RetryTimerMustFire);
    }
    if observation.result != ActionResultV1::RetryableFailure {
        return Err(EngineError::RetryTimerRequiresRetryableFailure);
    }
    let step = definition
        .steps
        .get(projection.next_step_index)
        .ok_or(EngineError::InvalidProjection)?;
    if step.retry_policy.backoff.is_none() {
        return Err(EngineError::RetryTimerNotConfigured);
    }
    let current_attempt = projection
        .current_attempt
        .checked_add(1)
        .ok_or(EngineError::AttemptOverflow)?;
    if current_attempt >= step.retry_policy.max_attempts.get() {
        return Err(EngineError::RetryTimerNotRequired);
    }
    let due_at = step
        .retry_policy
        .retry_due_at(observed_at, projection.current_attempt)?
        .ok_or(EngineError::RetryTimerNotConfigured)?;
    let action_id = fresh_action_id(projection, next_action_id)?;
    let timer = LinearSagaProjectionV1 {
        current_attempt,
        retry_due_at: Some(due_at),
        active_action_id: Some(action_id.clone()),
        issued_action_ids: issued_ids_after(projection, action_id),
        ..projection.clone()
    };
    Ok(SagaDecisionV1 {
        next_action: Some(action_for(definition, &timer, tenant_id, process_id)?),
        projection: timer,
    })
}

/// Delivers a due retry timer and unlocks the independently idempotent retry
/// effect action.
///
/// # Errors
///
/// Returns a typed invariant error for a mismatched, premature, duplicate, or
/// otherwise illegal timer delivery.
pub fn fire_retry_timer(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    timer_action_id: &ActionId,
    fired_at: LogicalTimeV1,
    next_action_id: ActionId,
) -> Result<SagaDecisionV1, EngineError> {
    definition.validate()?;
    if projection.definition_id != definition.definition_id
        || projection.definition_version != definition.definition_version
        || projection.definition_digest != definition.definition_digest
    {
        return Err(EngineError::DefinitionMismatch);
    }
    if projection.status != SagaStatusV1::Running {
        return Err(EngineError::TerminalProjection);
    }
    if projection.next_step_index >= definition.steps.len() {
        return Err(EngineError::InvalidProjection);
    }
    if projection.active_action_id.as_ref() != Some(timer_action_id) {
        return Err(EngineError::UnexpectedRetryTimer);
    }
    let due_at = projection
        .retry_due_at
        .ok_or(EngineError::UnexpectedRetryTimer)?;
    if fired_at < due_at {
        return Err(EngineError::RetryTimerFiredEarly);
    }
    let action_id = fresh_action_id(projection, Some(next_action_id))?;
    let retry = LinearSagaProjectionV1 {
        retry_due_at: None,
        active_action_id: Some(action_id.clone()),
        issued_action_ids: issued_ids_after(projection, action_id),
        ..projection.clone()
    };
    Ok(SagaDecisionV1 {
        next_action: Some(action_for(definition, &retry, tenant_id, process_id)?),
        projection: retry,
    })
}

/// Applies an already-authorized manual-review resolution to an escalated
/// process.
///
/// Authorization, attribution, and evidence retention are enforced by the
/// outer manual-review port before this pure transition is called. The accepted
/// resolution must be appended to the process log before dispatching any
/// returned action.
///
/// # Errors
///
/// Returns a typed invariant error when the process is not escalated, the
/// pinned definition differs, or a resolution requiring work lacks a fresh
/// action identity.
pub fn apply_manual_resolution(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    resolution: ManualReviewResolutionV1,
    next_action_id: Option<ActionId>,
) -> Result<SagaDecisionV1, EngineError> {
    definition.validate()?;
    if projection.definition_id != definition.definition_id
        || projection.definition_version != definition.definition_version
        || projection.definition_digest != definition.definition_digest
    {
        return Err(EngineError::DefinitionMismatch);
    }
    if projection.status != SagaStatusV1::Escalated {
        return Err(EngineError::ManualResolutionRequiresEscalation);
    }
    if projection.next_step_index >= definition.steps.len() {
        return Err(EngineError::InvalidProjection);
    }
    match resolution {
        ManualReviewResolutionV1::RetryAction => {
            let action_id = fresh_action_id(projection, next_action_id)?;
            let resumed = LinearSagaProjectionV1 {
                active_action_id: Some(action_id.clone()),
                issued_action_ids: issued_ids_after(projection, action_id),
                status: SagaStatusV1::Running,
                ..projection.clone()
            };
            Ok(SagaDecisionV1 {
                next_action: Some(action_for(definition, &resumed, tenant_id, process_id)?),
                projection: resumed,
            })
        }
        ManualReviewResolutionV1::Compensate => {
            if projection.compensable_step_indices.is_empty() {
                return Ok(SagaDecisionV1 {
                    projection: terminal_projection(
                        projection,
                        projection.next_step_index,
                        Vec::new(),
                        SagaStatusV1::Cancelled,
                    ),
                    next_action: None,
                });
            }
            let action_id = fresh_action_id(projection, next_action_id)?;
            let compensating = LinearSagaProjectionV1 {
                current_attempt: 0,
                active_action_id: Some(action_id.clone()),
                issued_action_ids: issued_ids_after(projection, action_id),
                status: SagaStatusV1::Compensating,
                ..projection.clone()
            };
            Ok(SagaDecisionV1 {
                next_action: Some(action_for(
                    definition,
                    &compensating,
                    tenant_id,
                    process_id,
                )?),
                projection: compensating,
            })
        }
        ManualReviewResolutionV1::Cancel => Ok(SagaDecisionV1 {
            projection: terminal_projection(
                projection,
                projection.next_step_index,
                projection.compensable_step_indices.clone(),
                SagaStatusV1::Cancelled,
            ),
            next_action: None,
        }),
        ManualReviewResolutionV1::Escalate => Ok(SagaDecisionV1 {
            projection: projection.clone(),
            next_action: None,
        }),
    }
}

fn apply_forward_result(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    result: ActionResultV1,
    next_action_id: Option<ActionId>,
) -> Result<SagaDecisionV1, EngineError> {
    match result {
        ActionResultV1::Succeeded => {
            let step = definition
                .steps
                .get(projection.next_step_index)
                .ok_or(EngineError::InvalidProjection)?;
            let mut compensable_step_indices = projection.compensable_step_indices.clone();
            if step.compensation.is_some() {
                compensable_step_indices.push(projection.next_step_index);
            }
            let next_step_index = projection.next_step_index.saturating_add(1);
            if next_step_index == definition.steps.len() {
                Ok(SagaDecisionV1 {
                    projection: terminal_projection(
                        projection,
                        next_step_index,
                        compensable_step_indices,
                        SagaStatusV1::Completed,
                    ),
                    next_action: None,
                })
            } else {
                let action_id = fresh_action_id(projection, next_action_id)?;
                let next = LinearSagaProjectionV1 {
                    next_step_index,
                    current_attempt: 0,
                    active_action_id: Some(action_id.clone()),
                    issued_action_ids: issued_ids_after(projection, action_id),
                    compensable_step_indices,
                    status: SagaStatusV1::Running,
                    ..projection.clone()
                };
                Ok(SagaDecisionV1 {
                    next_action: Some(action_for(definition, &next, tenant_id, process_id)?),
                    projection: next,
                })
            }
        }
        ActionResultV1::RetryableFailure => {
            let step = definition
                .steps
                .get(projection.next_step_index)
                .ok_or(EngineError::InvalidProjection)?;
            let current_attempt = projection
                .current_attempt
                .checked_add(1)
                .ok_or(EngineError::AttemptOverflow)?;
            if current_attempt >= step.retry_policy.max_attempts.get() {
                return begin_compensation(
                    definition,
                    projection,
                    tenant_id,
                    process_id,
                    next_action_id,
                );
            }
            if step.retry_policy.backoff.is_some() {
                return Err(EngineError::RetryTimerRequired);
            }
            let action_id = fresh_action_id(projection, next_action_id)?;
            let retry = LinearSagaProjectionV1 {
                next_step_index: projection.next_step_index,
                current_attempt,
                active_action_id: Some(action_id.clone()),
                issued_action_ids: issued_ids_after(projection, action_id),
                compensable_step_indices: projection.compensable_step_indices.clone(),
                status: SagaStatusV1::Running,
                ..projection.clone()
            };
            Ok(SagaDecisionV1 {
                next_action: Some(action_for(definition, &retry, tenant_id, process_id)?),
                projection: retry,
            })
        }
        ActionResultV1::TerminalFailure => begin_compensation(
            definition,
            projection,
            tenant_id,
            process_id,
            next_action_id,
        ),
        ActionResultV1::Unknown => Ok(SagaDecisionV1 {
            projection: terminal_projection(
                projection,
                projection.next_step_index,
                projection.compensable_step_indices.clone(),
                SagaStatusV1::Escalated,
            ),
            next_action: None,
        }),
    }
}

fn begin_compensation(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    next_action_id: Option<ActionId>,
) -> Result<SagaDecisionV1, EngineError> {
    if projection.compensable_step_indices.is_empty() {
        return Ok(SagaDecisionV1 {
            projection: terminal_projection(
                projection,
                projection.next_step_index,
                Vec::new(),
                SagaStatusV1::Escalated,
            ),
            next_action: None,
        });
    }
    let action_id = fresh_action_id(projection, next_action_id)?;
    let compensating = LinearSagaProjectionV1 {
        next_step_index: projection.next_step_index,
        current_attempt: 0,
        active_action_id: Some(action_id.clone()),
        issued_action_ids: issued_ids_after(projection, action_id),
        compensable_step_indices: projection.compensable_step_indices.clone(),
        status: SagaStatusV1::Compensating,
        ..projection.clone()
    };
    Ok(SagaDecisionV1 {
        next_action: Some(action_for(
            definition,
            &compensating,
            tenant_id,
            process_id,
        )?),
        projection: compensating,
    })
}

fn apply_compensation_result(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
    result: ActionResultV1,
    next_action_id: Option<ActionId>,
) -> Result<SagaDecisionV1, EngineError> {
    let compensation = compensation_for(definition, projection)?;
    match result {
        ActionResultV1::Succeeded => {
            let mut remaining = projection.compensable_step_indices.clone();
            let _ = remaining.pop();
            if remaining.is_empty() {
                return Ok(SagaDecisionV1 {
                    projection: terminal_projection(
                        projection,
                        projection.next_step_index,
                        remaining,
                        SagaStatusV1::Compensated,
                    ),
                    next_action: None,
                });
            }
            let action_id = fresh_action_id(projection, next_action_id)?;
            let next = LinearSagaProjectionV1 {
                next_step_index: projection.next_step_index,
                current_attempt: 0,
                active_action_id: Some(action_id.clone()),
                issued_action_ids: issued_ids_after(projection, action_id),
                compensable_step_indices: remaining,
                status: SagaStatusV1::Compensating,
                ..projection.clone()
            };
            Ok(SagaDecisionV1 {
                next_action: Some(action_for(definition, &next, tenant_id, process_id)?),
                projection: next,
            })
        }
        ActionResultV1::RetryableFailure => {
            let current_attempt = projection
                .current_attempt
                .checked_add(1)
                .ok_or(EngineError::AttemptOverflow)?;
            if current_attempt >= compensation.retry_policy.max_attempts.get() {
                return Ok(SagaDecisionV1 {
                    projection: terminal_projection(
                        projection,
                        projection.next_step_index,
                        projection.compensable_step_indices.clone(),
                        SagaStatusV1::Escalated,
                    ),
                    next_action: None,
                });
            }
            if compensation.retry_policy.backoff.is_some() {
                return Err(EngineError::RetryTimerRequired);
            }
            let action_id = fresh_action_id(projection, next_action_id)?;
            let retry = LinearSagaProjectionV1 {
                next_step_index: projection.next_step_index,
                current_attempt,
                active_action_id: Some(action_id.clone()),
                issued_action_ids: issued_ids_after(projection, action_id),
                compensable_step_indices: projection.compensable_step_indices.clone(),
                status: SagaStatusV1::Compensating,
                ..projection.clone()
            };
            Ok(SagaDecisionV1 {
                next_action: Some(action_for(definition, &retry, tenant_id, process_id)?),
                projection: retry,
            })
        }
        ActionResultV1::TerminalFailure | ActionResultV1::Unknown => Ok(SagaDecisionV1 {
            projection: terminal_projection(
                projection,
                projection.next_step_index,
                projection.compensable_step_indices.clone(),
                SagaStatusV1::Escalated,
            ),
            next_action: None,
        }),
    }
}

fn compensation_for(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
) -> Result<CompensationPlanV1, EngineError> {
    let step_index = projection
        .compensable_step_indices
        .last()
        .ok_or(EngineError::InvalidProjection)?;
    let step = definition
        .steps
        .get(*step_index)
        .ok_or(EngineError::InvalidProjection)?;
    step.compensation.ok_or(EngineError::InvalidProjection)
}

fn terminal_projection(
    projection: &LinearSagaProjectionV1,
    next_step_index: usize,
    compensable_step_indices: Vec<usize>,
    status: SagaStatusV1,
) -> LinearSagaProjectionV1 {
    LinearSagaProjectionV1 {
        next_step_index,
        current_attempt: projection.current_attempt,
        active_action_id: None,
        issued_action_ids: projection.issued_action_ids.clone(),
        compensable_step_indices,
        status,
        ..projection.clone()
    }
}

fn fresh_action_id(
    projection: &LinearSagaProjectionV1,
    next_action_id: Option<ActionId>,
) -> Result<ActionId, EngineError> {
    let action_id = next_action_id.ok_or(EngineError::MissingNextAction)?;
    if projection.issued_action_ids.contains(&action_id) {
        return Err(EngineError::ReusedActionId);
    }
    Ok(action_id)
}

fn issued_ids_after(projection: &LinearSagaProjectionV1, action_id: ActionId) -> Vec<ActionId> {
    let mut issued_action_ids = projection.issued_action_ids.clone();
    issued_action_ids.push(action_id);
    issued_action_ids
}

/// Rebuilds the current decision from an ordered immutable process log.
///
/// The returned action, if any, is only the final pending action. Calling this
/// function does not perform I/O or authorize a dispatcher to resend an
/// earlier effect; an outer durable worker must reconcile and dispatch it.
///
/// # Errors
///
/// Returns an invariant error if the log lacks a start event, has multiple
/// starts, or contains an invalid action transition.
pub fn replay(
    definition: &LinearSagaDefinitionV1,
    tenant_id: &TenantId,
    process_id: &ProcessId,
    events: &[LinearSagaEventV1],
) -> Result<SagaDecisionV1, EngineError> {
    let mut decision: Option<SagaDecisionV1> = None;
    for event in events {
        match event {
            LinearSagaEventV1::Started {
                definition_id,
                definition_version,
                definition_digest,
                action_id,
            } => {
                if decision.is_some() {
                    return Err(EngineError::DuplicateStart);
                }
                if definition_id != &definition.definition_id
                    || definition_version != &definition.definition_version
                    || definition_digest != &definition.definition_digest
                {
                    return Err(EngineError::DefinitionMismatch);
                }
                decision = Some(start(
                    definition,
                    tenant_id.clone(),
                    process_id.clone(),
                    action_id.clone(),
                )?);
            }
            LinearSagaEventV1::ActionResultObserved {
                observation,
                next_action_id,
            } => {
                let current = decision.as_ref().ok_or(EngineError::MissingStart)?;
                decision = Some(apply_action_result(
                    definition,
                    &current.projection,
                    tenant_id.clone(),
                    process_id.clone(),
                    observation,
                    next_action_id.clone(),
                )?);
            }
            LinearSagaEventV1::RetryTimerScheduled {
                observation,
                next_action_id,
                observed_at,
            } => {
                let current = decision.as_ref().ok_or(EngineError::MissingStart)?;
                decision = Some(schedule_retry_timer(
                    definition,
                    &current.projection,
                    tenant_id.clone(),
                    process_id.clone(),
                    observation,
                    next_action_id.clone(),
                    *observed_at,
                )?);
            }
            LinearSagaEventV1::RetryTimerFired {
                timer_action_id,
                fired_at,
                next_action_id,
            } => {
                let current = decision.as_ref().ok_or(EngineError::MissingStart)?;
                decision = Some(fire_retry_timer(
                    definition,
                    &current.projection,
                    tenant_id.clone(),
                    process_id.clone(),
                    timer_action_id,
                    *fired_at,
                    next_action_id.clone(),
                )?);
            }
            LinearSagaEventV1::ManualResolutionApplied {
                resolution,
                next_action_id,
            } => {
                let current = decision.as_ref().ok_or(EngineError::MissingStart)?;
                decision = Some(apply_manual_resolution(
                    definition,
                    &current.projection,
                    tenant_id.clone(),
                    process_id.clone(),
                    *resolution,
                    next_action_id.clone(),
                )?);
            }
        }
    }
    decision.ok_or(EngineError::MissingStart)
}

/// Rebuilds a decision after validating a contiguous ordered event log.
///
/// The first event must have sequence zero and every following event must
/// increment by one. This function does not write, dispatch, or otherwise
/// mutate external state.
///
/// # Errors
///
/// Returns a typed sequence error before evaluating an invalid log, or an
/// engine invariant error from [`replay`] for invalid event semantics.
pub fn replay_ordered(
    definition: &LinearSagaDefinitionV1,
    tenant_id: &TenantId,
    process_id: &ProcessId,
    envelopes: &[LinearSagaEventEnvelopeV1],
) -> Result<SagaDecisionV1, EngineError> {
    let mut expected_sequence = 0_u64;
    let mut events = Vec::with_capacity(envelopes.len());
    for envelope in envelopes {
        if envelope.sequence != expected_sequence {
            return Err(EngineError::InvalidEventSequence);
        }
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or(EngineError::EventSequenceOverflow)?;
        events.push(envelope.event.clone());
    }
    replay(definition, tenant_id, process_id, &events)
}

fn action_for(
    definition: &LinearSagaDefinitionV1,
    projection: &LinearSagaProjectionV1,
    tenant_id: TenantId,
    process_id: ProcessId,
) -> Result<ProcessActionDtoV1, EngineError> {
    let (step_id, action_kind, payload_digest) = if projection.retry_due_at.is_some() {
        let step = definition
            .steps
            .get(projection.next_step_index)
            .ok_or(EngineError::InvalidProjection)?;
        (
            step.step_id.clone(),
            ProcessActionKindV1::Timer,
            step.payload_digest,
        )
    } else if projection.status == SagaStatusV1::Compensating {
        let step_index = projection
            .compensable_step_indices
            .last()
            .ok_or(EngineError::InvalidProjection)?;
        let step = definition
            .steps
            .get(*step_index)
            .ok_or(EngineError::InvalidProjection)?;
        let compensation = step.compensation.ok_or(EngineError::InvalidProjection)?;
        (
            step.step_id.clone(),
            compensation.action_kind,
            compensation.payload_digest,
        )
    } else {
        let step = definition
            .steps
            .get(projection.next_step_index)
            .ok_or(EngineError::InvalidProjection)?;
        (step.step_id.clone(), step.action_kind, step.payload_digest)
    };
    Ok(ProcessActionDtoV1::new(
        ProcessScopeV1::new(
            tenant_id,
            process_id,
            projection.definition_id.clone(),
            projection.definition_version.clone(),
            projection.definition_digest,
        ),
        projection
            .active_action_id
            .clone()
            .ok_or(EngineError::InvalidProjection)?,
        step_id,
        projection.current_attempt,
        action_kind,
        payload_digest,
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
        T::try_from(value).ok().unwrap()
    }

    fn definition() -> LinearSagaDefinitionV1 {
        let retry_policy = RetryPolicyV1::new(NonZeroU32::new(2).unwrap());
        LinearSagaDefinitionV1 {
            definition_id: id("def_trade"),
            definition_version: id("dfv_one"),
            definition_digest: ContentDigest([99; 32]),
            steps: vec![
                StepPlanV1 {
                    step_id: id("stp_lock"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([1; 32]),
                    retry_policy,
                    compensation: None,
                },
                StepPlanV1 {
                    step_id: id("stp_settle"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([2; 32]),
                    retry_policy,
                    compensation: None,
                },
            ],
        }
    }

    #[test]
    fn linear_definition_rejects_duplicate_and_oversized_steps() {
        let policy = RetryPolicyV1::no_retry();
        let duplicate = LinearSagaDefinitionV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([99; 32]),
            vec![
                StepPlanV1::canonical_command(id("stp_lock"), ContentDigest([1; 32]), policy),
                StepPlanV1::canonical_command(id("stp_lock"), ContentDigest([2; 32]), policy),
            ],
        );
        assert_eq!(
            duplicate.validate().unwrap_err(),
            EngineError::DuplicateStepId
        );
        let oversized = LinearSagaDefinitionV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([99; 32]),
            vec![
                StepPlanV1::canonical_command(id("stp_limit"), ContentDigest([3; 32]), policy);
                MAX_DEFINITION_STEPS.saturating_add(1)
            ],
        );
        assert_eq!(
            oversized.validate().unwrap_err(),
            EngineError::DefinitionStepLimitExceeded
        );
    }

    fn observation(
        action: &ProcessActionDtoV1,
        result: ActionResultV1,
    ) -> ActionResultObservationV1 {
        ActionResultObservationV1 {
            action_id: action.action_id.clone(),
            result,
        }
    }

    #[test]
    fn success_plans_ordered_steps_then_completes() {
        let definition = definition();
        let first = start(&definition, id("tnt_game"), id("prc_trade"), id("act_lock")).unwrap();
        assert_eq!(first.next_action.as_ref().unwrap().step_id, id("stp_lock"));
        assert_eq!(
            first.next_action.as_ref().unwrap().definition_digest,
            definition.definition_digest
        );
        let second = apply_action_result(
            &definition,
            &first.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                first.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            Some(id("act_settle")),
        )
        .unwrap();
        assert_eq!(
            second.next_action.as_ref().unwrap().step_id,
            id("stp_settle")
        );
        let complete = apply_action_result(
            &definition,
            &second.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                second.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            None,
        )
        .unwrap();
        assert_eq!(complete.projection.status, SagaStatusV1::Completed);
    }

    #[test]
    fn decide_routes_typed_start_and_result_inputs_without_side_effects() {
        let definition = definition();
        let started = decide(
            &definition,
            None,
            id("tnt_game"),
            id("prc_trade"),
            &LinearSagaInputV1::Start {
                action_id: id("act_lock"),
            },
        )
        .unwrap();
        let advanced = decide(
            &definition,
            Some(&started.projection),
            id("tnt_game"),
            id("prc_trade"),
            &LinearSagaInputV1::ActionResult {
                observation: ActionResultObservationV1::succeeded(
                    started.next_action.as_ref().unwrap().action_id.clone(),
                ),
                next_action_id: Some(id("act_settle")),
            },
        )
        .unwrap();
        assert_eq!(
            advanced.next_action.as_ref().unwrap().step_id,
            id("stp_settle")
        );
    }

    #[test]
    fn decide_rejects_a_result_without_a_replayed_projection() {
        let error = decide(
            &definition(),
            None,
            id("tnt_game"),
            id("prc_trade"),
            &LinearSagaInputV1::ActionResult {
                observation: ActionResultObservationV1::succeeded(id("act_lock")),
                next_action_id: Some(id("act_settle")),
            },
        )
        .unwrap_err();
        assert_eq!(error, EngineError::MissingProjection);
    }

    #[test]
    fn retry_backoff_is_deterministic_exponential_and_capped() {
        let backoff =
            RetryBackoffV1::new(NonZeroU64::new(10).unwrap(), NonZeroU64::new(40).unwrap())
                .unwrap();
        let policy = RetryPolicyV1::new(NonZeroU32::new(5).unwrap()).with_backoff(backoff);
        assert_eq!(
            policy.retry_due_at(LogicalTimeV1(100), 0).unwrap(),
            Some(LogicalTimeV1(110))
        );
        assert_eq!(
            policy.retry_due_at(LogicalTimeV1(100), 1).unwrap(),
            Some(LogicalTimeV1(120))
        );
        assert_eq!(
            policy.retry_due_at(LogicalTimeV1(100), 2).unwrap(),
            Some(LogicalTimeV1(140))
        );
        assert_eq!(
            policy.retry_due_at(LogicalTimeV1(100), 31).unwrap(),
            Some(LogicalTimeV1(140))
        );
    }

    #[test]
    fn retry_backoff_rejects_invalid_bounds_and_due_time_overflow() {
        let invalid = RetryBackoffV1::new(NonZeroU64::new(2).unwrap(), NonZeroU64::new(1).unwrap());
        assert_eq!(invalid.unwrap_err(), EngineError::InvalidRetryBackoff);

        let backoff =
            RetryBackoffV1::new(NonZeroU64::new(1).unwrap(), NonZeroU64::new(1).unwrap()).unwrap();
        let policy = RetryPolicyV1::new(NonZeroU32::MIN).with_backoff(backoff);
        assert_eq!(
            policy.retry_due_at(LogicalTimeV1(u64::MAX), 0),
            Err(EngineError::RetryDueTimeOverflow)
        );
    }

    #[test]
    fn retry_backoff_plans_replayable_due_timer_and_rejects_early_or_duplicate_fire() {
        let backoff =
            RetryBackoffV1::new(NonZeroU64::new(10).unwrap(), NonZeroU64::new(100).unwrap())
                .unwrap();
        let definition = LinearSagaDefinitionV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([99; 32]),
            vec![StepPlanV1::canonical_command(
                id("stp_lock"),
                ContentDigest([1; 32]),
                RetryPolicyV1::new(NonZeroU32::new(3).unwrap()).with_backoff(backoff),
            )],
        );
        let tenant_id = id::<TenantId>("tnt_game");
        let process_id = id::<ProcessId>("prc_trade");
        let started = start(
            &definition,
            tenant_id.clone(),
            process_id.clone(),
            id("act_first"),
        )
        .unwrap();
        let failure = observation(
            started.next_action.as_ref().unwrap(),
            ActionResultV1::RetryableFailure,
        );
        let scheduled = schedule_retry_timer(
            &definition,
            &started.projection,
            tenant_id.clone(),
            process_id.clone(),
            &failure,
            Some(id("act_retry_timer")),
            LogicalTimeV1(100),
        )
        .unwrap();
        let timer = scheduled.next_action.as_ref().unwrap();
        assert_eq!(timer.kind, ProcessActionKindV1::Timer);
        assert_eq!(
            scheduled.retry_timer_schedule().unwrap().due_at,
            LogicalTimeV1(110)
        );
        assert_eq!(
            apply_action_result(
                &definition,
                &scheduled.projection,
                tenant_id.clone(),
                process_id.clone(),
                &ActionResultObservationV1::succeeded(timer.action_id.clone()),
                Some(id("act_wrong")),
            )
            .unwrap_err(),
            EngineError::RetryTimerMustFire
        );
        assert_eq!(
            fire_retry_timer(
                &definition,
                &scheduled.projection,
                tenant_id.clone(),
                process_id.clone(),
                &timer.action_id,
                LogicalTimeV1(109),
                id("act_retry"),
            )
            .unwrap_err(),
            EngineError::RetryTimerFiredEarly
        );
        let retry = fire_retry_timer(
            &definition,
            &scheduled.projection,
            tenant_id.clone(),
            process_id.clone(),
            &timer.action_id,
            LogicalTimeV1(110),
            id("act_retry"),
        )
        .unwrap();
        assert_eq!(
            retry.next_action.as_ref().unwrap().kind,
            ProcessActionKindV1::CanonicalCommand
        );
        assert_eq!(retry.next_action.as_ref().unwrap().attempt, 1);
        assert_eq!(
            fire_retry_timer(
                &definition,
                &retry.projection,
                tenant_id.clone(),
                process_id.clone(),
                &timer.action_id,
                LogicalTimeV1(110),
                id("act_retry_duplicate"),
            )
            .unwrap_err(),
            EngineError::UnexpectedRetryTimer
        );
        let events = [
            LinearSagaEventV1::started(&definition, id("act_first")),
            LinearSagaEventV1::RetryTimerScheduled {
                observation: failure,
                next_action_id: Some(id("act_retry_timer")),
                observed_at: LogicalTimeV1(100),
            },
            LinearSagaEventV1::RetryTimerFired {
                timer_action_id: id("act_retry_timer"),
                fired_at: LogicalTimeV1(110),
                next_action_id: id("act_retry"),
            },
        ];
        assert_eq!(
            replay(&definition, &tenant_id, &process_id, &events).unwrap(),
            retry
        );
    }

    #[test]
    fn authorized_manual_resolution_replays_compensation_or_cancellation() {
        let policy = RetryPolicyV1::no_retry();
        let definition = LinearSagaDefinitionV1::new(
            id("def_trade"),
            id("dfv_one"),
            ContentDigest([99; 32]),
            vec![
                StepPlanV1::canonical_command(id("stp_lock"), ContentDigest([1; 32]), policy)
                    .with_compensation(CompensationPlanV1::canonical_command(
                        ContentDigest([9; 32]),
                        policy,
                    )),
                StepPlanV1::canonical_command(id("stp_settle"), ContentDigest([2; 32]), policy),
            ],
        );
        let tenant_id = id::<TenantId>("tnt_market");
        let process_id = id::<ProcessId>("prc_trade");
        let started = start(
            &definition,
            tenant_id.clone(),
            process_id.clone(),
            id("act_lock"),
        )
        .unwrap();
        let settle = apply_action_result(
            &definition,
            &started.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            Some(id("act_settle")),
        )
        .unwrap();
        let escalated = apply_action_result(
            &definition,
            &settle.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observation(
                settle.next_action.as_ref().unwrap(),
                ActionResultV1::Unknown,
            ),
            None,
        )
        .unwrap();
        assert_eq!(escalated.projection.status, SagaStatusV1::Escalated);
        let compensation = apply_manual_resolution(
            &definition,
            &escalated.projection,
            tenant_id.clone(),
            process_id.clone(),
            ManualReviewResolutionV1::Compensate,
            Some(id("act_unlock")),
        )
        .unwrap();
        assert_eq!(compensation.projection.status, SagaStatusV1::Compensating);
        assert_eq!(
            compensation.next_action.as_ref().unwrap().step_id,
            id("stp_lock")
        );
        let cancelled = apply_manual_resolution(
            &definition,
            &escalated.projection,
            tenant_id.clone(),
            process_id.clone(),
            ManualReviewResolutionV1::Cancel,
            None,
        )
        .unwrap();
        assert_eq!(cancelled.projection.status, SagaStatusV1::Cancelled);
        assert_eq!(
            apply_manual_resolution(
                &definition,
                &settle.projection,
                tenant_id.clone(),
                process_id.clone(),
                ManualReviewResolutionV1::Cancel,
                None,
            )
            .unwrap_err(),
            EngineError::ManualResolutionRequiresEscalation
        );
        let events = [
            LinearSagaEventV1::started(&definition, id("act_lock")),
            LinearSagaEventV1::ActionResultObserved {
                observation: ActionResultObservationV1::succeeded(id("act_lock")),
                next_action_id: Some(id("act_settle")),
            },
            LinearSagaEventV1::ActionResultObserved {
                observation: ActionResultObservationV1::unknown(id("act_settle")),
                next_action_id: None,
            },
            LinearSagaEventV1::ManualResolutionApplied {
                resolution: ManualReviewResolutionV1::Compensate,
                next_action_id: Some(id("act_unlock")),
            },
        ];
        assert_eq!(
            replay(&definition, &tenant_id, &process_id, &events).unwrap(),
            compensation
        );
    }

    #[test]
    fn unknown_result_escalates_instead_of_blind_retry() {
        let started = start(
            &definition(),
            id("tnt_game"),
            id("prc_trade"),
            id("act_lock"),
        )
        .unwrap();
        let decision = apply_action_result(
            &definition(),
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::Unknown,
            ),
            None,
        )
        .unwrap();
        assert_eq!(decision.projection.status, SagaStatusV1::Escalated);
        assert!(decision.next_action.is_none());
    }

    #[test]
    fn retry_creates_a_new_attempt_for_the_same_step() {
        let started = start(
            &definition(),
            id("tnt_game"),
            id("prc_trade"),
            id("act_lock"),
        )
        .unwrap();
        let retry = apply_action_result(
            &definition(),
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::RetryableFailure,
            ),
            Some(id("act_lock_retry")),
        )
        .unwrap();
        assert_eq!(retry.projection.current_attempt, 1);
        assert_eq!(retry.next_action.as_ref().unwrap().attempt, 1);
        assert_eq!(retry.next_action.as_ref().unwrap().step_id, id("stp_lock"));
    }

    #[test]
    fn exhausted_retry_policy_escalates_without_a_new_action() {
        let definition = LinearSagaDefinitionV1 {
            definition_id: id("def_trade"),
            definition_version: id("dfv_one"),
            definition_digest: ContentDigest([99; 32]),
            steps: vec![StepPlanV1::canonical_command(
                id("stp_lock"),
                ContentDigest([1; 32]),
                RetryPolicyV1::no_retry(),
            )],
        };
        let started = start(&definition, id("tnt_game"), id("prc_trade"), id("act_lock")).unwrap();
        let decision = apply_action_result(
            &definition,
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::RetryableFailure,
            ),
            Some(id("act_retry")),
        )
        .unwrap();
        assert_eq!(decision.projection.status, SagaStatusV1::Escalated);
        assert!(decision.next_action.is_none());
    }

    #[test]
    fn known_terminal_failure_compensates_succeeded_steps_in_lifo_order() {
        let policy = RetryPolicyV1::no_retry();
        let definition = LinearSagaDefinitionV1 {
            definition_id: id("def_trade"),
            definition_version: id("dfv_one"),
            definition_digest: ContentDigest([99; 32]),
            steps: vec![
                StepPlanV1 {
                    step_id: id("stp_lock_a"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([1; 32]),
                    retry_policy: policy,
                    compensation: Some(CompensationPlanV1::canonical_command(
                        ContentDigest([10; 32]),
                        policy,
                    )),
                },
                StepPlanV1 {
                    step_id: id("stp_lock_b"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([2; 32]),
                    retry_policy: policy,
                    compensation: Some(CompensationPlanV1::canonical_command(
                        ContentDigest([11; 32]),
                        policy,
                    )),
                },
                StepPlanV1::canonical_command(id("stp_settle"), ContentDigest([3; 32]), policy),
            ],
        };
        let tenant_id = id::<TenantId>("tnt_game");
        let process_id = id::<ProcessId>("prc_trade");
        let first = start(
            &definition,
            tenant_id.clone(),
            process_id.clone(),
            id("act_lock_a"),
        )
        .unwrap();
        let second = apply_action_result(
            &definition,
            &first.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observation(
                first.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            Some(id("act_lock_b")),
        )
        .unwrap();
        let settlement = apply_action_result(
            &definition,
            &second.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observation(
                second.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            Some(id("act_settle")),
        )
        .unwrap();
        let unlock_b = apply_action_result(
            &definition,
            &settlement.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observation(
                settlement.next_action.as_ref().unwrap(),
                ActionResultV1::TerminalFailure,
            ),
            Some(id("act_unlock_b")),
        )
        .unwrap();
        assert_eq!(unlock_b.projection.status, SagaStatusV1::Compensating);
        assert_eq!(
            unlock_b.next_action.as_ref().unwrap().step_id,
            id("stp_lock_b")
        );
        assert_eq!(
            unlock_b.next_action.as_ref().unwrap().payload_digest,
            ContentDigest([11; 32])
        );
        let unlock_a = apply_action_result(
            &definition,
            &unlock_b.projection,
            tenant_id.clone(),
            process_id.clone(),
            &observation(
                unlock_b.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            Some(id("act_unlock_a")),
        )
        .unwrap();
        assert_eq!(
            unlock_a.next_action.as_ref().unwrap().step_id,
            id("stp_lock_a")
        );
        assert_eq!(
            unlock_a.next_action.as_ref().unwrap().payload_digest,
            ContentDigest([10; 32])
        );
        let compensated = apply_action_result(
            &definition,
            &unlock_a.projection,
            tenant_id,
            process_id,
            &observation(
                unlock_a.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            None,
        )
        .unwrap();
        assert_eq!(compensated.projection.status, SagaStatusV1::Compensated);
        assert!(compensated.next_action.is_none());
    }

    #[test]
    fn crash_after_every_trade_event_replays_to_the_same_compensated_state() {
        let policy = RetryPolicyV1::no_retry();
        let definition = LinearSagaDefinitionV1 {
            definition_id: id("def_trade"),
            definition_version: id("dfv_one"),
            definition_digest: ContentDigest([99; 32]),
            steps: vec![
                StepPlanV1 {
                    step_id: id("stp_lock"),
                    action_kind: ProcessActionKindV1::CanonicalCommand,
                    payload_digest: ContentDigest([1; 32]),
                    retry_policy: policy,
                    compensation: Some(CompensationPlanV1::canonical_command(
                        ContentDigest([9; 32]),
                        policy,
                    )),
                },
                StepPlanV1::canonical_command(id("stp_settle"), ContentDigest([2; 32]), policy),
            ],
        };
        let tenant_id = id::<TenantId>("tnt_game");
        let process_id = id::<ProcessId>("prc_trade");
        let events = vec![
            LinearSagaEventV1::started(&definition, id("act_lock")),
            LinearSagaEventV1::ActionResultObserved {
                observation: ActionResultObservationV1::succeeded(id("act_lock")),
                next_action_id: Some(id("act_settle")),
            },
            LinearSagaEventV1::ActionResultObserved {
                observation: ActionResultObservationV1 {
                    action_id: id("act_settle"),
                    result: ActionResultV1::TerminalFailure,
                },
                next_action_id: Some(id("act_unlock")),
            },
            LinearSagaEventV1::ActionResultObserved {
                observation: ActionResultObservationV1::succeeded(id("act_unlock")),
                next_action_id: None,
            },
        ];

        for prefix_length in 1..=events.len() {
            let prefix: Vec<_> = events.iter().take(prefix_length).cloned().collect();
            let restarted_once = replay(&definition, &tenant_id, &process_id, &prefix).unwrap();
            let restarted_twice = replay(&definition, &tenant_id, &process_id, &prefix).unwrap();
            assert_eq!(restarted_once, restarted_twice);
        }

        let final_decision = replay(&definition, &tenant_id, &process_id, &events).unwrap();
        assert_eq!(final_decision.projection.status, SagaStatusV1::Compensated);
        assert!(final_decision.next_action.is_none());
    }

    proptest! {
        #[test]
        fn retry_attempt_is_monotonic_for_every_representable_attempt(
            current_attempt in 0_u32..u32::MAX,
        ) {
            let Some(expected_attempt) = current_attempt.checked_add(1) else {
                return Ok(());
            };
            let mut definition = definition();
            let Some(step) = definition.steps.first_mut() else {
                return Ok(());
            };
            step.retry_policy = RetryPolicyV1::new(NonZeroU32::new(u32::MAX).unwrap());
            let retry = apply_action_result(
                &definition,
                &LinearSagaProjectionV1 {
                    definition_id: definition.definition_id.clone(),
                    definition_version: definition.definition_version.clone(),
                    definition_digest: definition.definition_digest,
                    next_step_index: 0,
                    current_attempt,
                    retry_due_at: None,
                    active_action_id: Some(id("act_lock")),
                    issued_action_ids: vec![id("act_lock")],
                    compensable_step_indices: Vec::new(),
                    status: SagaStatusV1::Running,
                },
                id("tnt_game"),
                id("prc_trade"),
                &ActionResultObservationV1 {
                    action_id: id("act_lock"),
                    result: ActionResultV1::RetryableFailure,
                },
                Some(id("act_lock_retry")),
            )
            .unwrap();
            prop_assert_eq!(retry.projection.next_step_index, 0);
            prop_assert_eq!(retry.projection.current_attempt, expected_attempt);
            prop_assert_eq!(retry.next_action.unwrap().attempt, expected_attempt);
        }

        #[test]
        fn live_transitions_and_event_replay_converge(
            result_codes in proptest::collection::vec(0_u8..4, 0..5),
        ) {
            let mut definition = definition();
            let Some(step) = definition.steps.first_mut() else {
                return Ok(());
            };
            step.retry_policy = RetryPolicyV1::new(NonZeroU32::new(u32::MAX).unwrap());
            let tenant_id = id::<TenantId>("tnt_game");
            let process_id = id::<ProcessId>("prc_trade");
            let start_action_id = id::<ActionId>("act_start");
            let mut action_ids = [
                id::<ActionId>("act_one"),
                id::<ActionId>("act_two"),
                id::<ActionId>("act_three"),
                id::<ActionId>("act_four"),
                id::<ActionId>("act_five"),
            ]
            .into_iter();
            let mut live = start(
                &definition,
                tenant_id.clone(),
                process_id.clone(),
                start_action_id.clone(),
            )
            .unwrap();
            let mut events = vec![LinearSagaEventV1::started(&definition, start_action_id)];
            let restarted_after_start = replay(&definition, &tenant_id, &process_id, &events).unwrap();
            prop_assert_eq!(&restarted_after_start, &live);

            for code in result_codes {
                let Some(action) = live.next_action.as_ref() else {
                    break;
                };
                let result = match code {
                    0 => ActionResultV1::Succeeded,
                    1 => ActionResultV1::RetryableFailure,
                    2 => ActionResultV1::TerminalFailure,
                    _ => ActionResultV1::Unknown,
                };
                let observation = ActionResultObservationV1 {
                    action_id: action.action_id.clone(),
                    result,
                };
                let next_action_id = match result {
                    ActionResultV1::Succeeded | ActionResultV1::RetryableFailure => {
                        action_ids.next()
                    }
                    ActionResultV1::TerminalFailure | ActionResultV1::Unknown => None,
                };
                if matches!(result, ActionResultV1::Succeeded | ActionResultV1::RetryableFailure)
                    && next_action_id.is_none()
                {
                    break;
                }
                events.push(LinearSagaEventV1::ActionResultObserved {
                    observation: observation.clone(),
                    next_action_id: next_action_id.clone(),
                });
                live = apply_action_result(
                    &definition,
                    &live.projection,
                    tenant_id.clone(),
                    process_id.clone(),
                    &observation,
                    next_action_id,
                )
                .unwrap();
                let restarted_after_event =
                    replay(&definition, &tenant_id, &process_id, &events).unwrap();
                prop_assert_eq!(&restarted_after_event, &live);
            }

            let replayed = replay(&definition, &tenant_id, &process_id, &events).unwrap();
            prop_assert_eq!(replayed, live);
        }
    }

    #[test]
    fn result_for_a_different_action_cannot_advance_the_saga() {
        let started = start(
            &definition(),
            id("tnt_game"),
            id("prc_trade"),
            id("act_lock"),
        )
        .unwrap();
        let error = apply_action_result(
            &definition(),
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &ActionResultObservationV1 {
                action_id: id("act_other"),
                result: ActionResultV1::Succeeded,
            },
            Some(id("act_settle")),
        )
        .unwrap_err();
        assert_eq!(error, EngineError::UnexpectedAction);
    }

    #[test]
    fn a_projection_rejects_a_definition_with_different_pinned_semantics() {
        let definition = definition();
        let started = start(&definition, id("tnt_game"), id("prc_trade"), id("act_lock")).unwrap();
        let mut changed_definition = definition;
        changed_definition.definition_digest = ContentDigest([42; 32]);
        let error = apply_action_result(
            &changed_definition,
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::Succeeded,
            ),
            Some(id("act_settle")),
        )
        .unwrap_err();
        assert_eq!(error, EngineError::DefinitionMismatch);
    }

    #[test]
    fn retry_cannot_reuse_an_issued_action_identity() {
        let started = start(
            &definition(),
            id("tnt_game"),
            id("prc_trade"),
            id("act_lock"),
        )
        .unwrap();
        let error = apply_action_result(
            &definition(),
            &started.projection,
            id("tnt_game"),
            id("prc_trade"),
            &observation(
                started.next_action.as_ref().unwrap(),
                ActionResultV1::RetryableFailure,
            ),
            Some(id("act_lock")),
        )
        .unwrap_err();
        assert_eq!(error, EngineError::ReusedActionId);
    }

    #[test]
    fn replay_rebuilds_the_final_projection_from_immutable_events() {
        let definition = definition();
        let replayed = replay(
            &definition,
            &id("tnt_game"),
            &id("prc_trade"),
            &[
                LinearSagaEventV1::started(&definition, id("act_lock")),
                LinearSagaEventV1::ActionResultObserved {
                    observation: ActionResultObservationV1::succeeded(id("act_lock")),
                    next_action_id: Some(id("act_settle")),
                },
                LinearSagaEventV1::ActionResultObserved {
                    observation: ActionResultObservationV1::succeeded(id("act_settle")),
                    next_action_id: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(replayed.projection.status, SagaStatusV1::Completed);
        assert!(replayed.next_action.is_none());
    }

    #[test]
    fn replay_rejects_a_second_start_record() {
        let definition = definition();
        let error = replay(
            &definition,
            &id("tnt_game"),
            &id("prc_trade"),
            &[
                LinearSagaEventV1::started(&definition, id("act_lock")),
                LinearSagaEventV1::started(&definition, id("act_other")),
            ],
        )
        .unwrap_err();
        assert_eq!(error, EngineError::DuplicateStart);
    }

    #[test]
    fn replay_rejects_a_start_event_with_different_pinned_semantics() {
        let definition = definition();
        let event = LinearSagaEventV1::started(&definition, id("act_lock"));
        let mut changed_definition = definition;
        changed_definition.definition_digest = ContentDigest([42; 32]);
        let error = replay(
            &changed_definition,
            &id("tnt_game"),
            &id("prc_trade"),
            &[event],
        )
        .unwrap_err();
        assert_eq!(error, EngineError::DefinitionMismatch);
    }

    #[test]
    fn ordered_replay_rejects_a_sequence_gap_before_applying_events() {
        let definition = definition();
        let error = replay_ordered(
            &definition,
            &id("tnt_game"),
            &id("prc_trade"),
            &[LinearSagaEventEnvelopeV1 {
                sequence: 1,
                event: LinearSagaEventV1::started(&definition, id("act_lock")),
            }],
        )
        .unwrap_err();
        assert_eq!(error, EngineError::InvalidEventSequence);
    }
}
