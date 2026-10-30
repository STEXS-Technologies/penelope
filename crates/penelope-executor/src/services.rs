//! Bounded integration calls. The host owns scheduling, authenticated principal
//! identity and trusted clocks; these functions neither create workers nor loop
//! indefinitely. Low-level composition APIs still require boundary authorization.
use crate::{
    composition::{CommittedTaskEffect, TaskCompositionError},
    task::TaskSupervisor,
};
use penelope_core::task::{
    TaskGeneration, TaskStatus,
    runtime::{TaskClock, TaskJournal, TaskRuntime},
};
use penelope_domain::{
    CanonicalWireBytes, ContentDigest, LogicalTime, PrincipalId, ProcessInput, ProcessInputKind,
    ProcessScope,
};
use penelope_ports::{payloads::ProcessPayloadStore, *};
use std::fmt::Write as _;

async fn authorize(
    authorizer: &impl ProcessAuthorizer,
    scope: &ProcessScope,
    principal: &PrincipalId,
    operation: ProcessAuthorizationOperation,
) -> Result<(), PortError> {
    authorizer
        .authorize(&ProcessAuthorizationRequest {
            scope: scope.clone(),
            principal_id: principal.clone(),
            operation,
        })
        .await?
        .require_authorized()
}

/// Claims one bounded batch only after authorizing its exact scope and owner.
/// # Errors
/// Denial performs no claim. Invalid or duplicate returned leases fail closed;
/// already claimed records remain leased until expiry and must not be dispatched.
pub async fn claim_outbox(
    authorizer: &impl ProcessAuthorizer,
    outbox: &impl OutboxStore,
    request: &OutboxClaimRequest,
    now: LogicalTime,
) -> Result<Vec<OutboxLease>, PortError> {
    request.validate()?;
    authorize(
        authorizer,
        &request.scope,
        &request.owner,
        ProcessAuthorizationOperation::Dispatch,
    )
    .await?;
    let leases = outbox.claim(request).await?;
    if leases.len() > usize::from(request.limit.get()) {
        return Err(PortError::Invariant);
    }
    for (index, lease) in leases.iter().enumerate() {
        lease.validate_for_claim_at(request, now)?;
        if lease.record.acknowledgement != OutboxAcknowledgement::Pending
            || leases.iter().skip(index.saturating_add(1)).any(|other| {
                other.record.action.action_id == lease.record.action.action_id
                    || other.token == lease.token
            })
        {
            return Err(PortError::Invariant);
        }
    }
    Ok(leases)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskDriveReport {
    pub changed: bool,
    pub status: TaskStatus,
    pub generation: TaskGeneration,
}

/// Makes at most one task transition/poll after exact scope authorization.
/// # Errors
/// Returns authorization, binding, journal, runtime and recovery failures.
pub async fn drive_task_once<J: TaskJournal, R: TaskRuntime, const N: usize>(
    authorizer: &impl ProcessAuthorizer,
    principal: &PrincipalId,
    committed: &CommittedTaskEffect,
    supervisor: &mut TaskSupervisor<J, R, N>,
    clock: &impl TaskClock,
) -> Result<TaskDriveReport, TaskCompositionError> {
    let binding = committed.binding();
    if supervisor.projection().scope() != binding.scope()
        || supervisor.projection().definition() != binding.definition()
    {
        return Err(TaskCompositionError::BindingMismatch);
    }
    authorize(
        authorizer,
        &binding.request().action.scope(),
        principal,
        ProcessAuthorizationOperation::DriveTask,
    )
    .await?;
    let tick = clock.now();
    let changed = if supervisor.projection().status() == TaskStatus::Running {
        supervisor.poll(tick)?
    } else {
        supervisor.advance(tick)?
    };
    Ok(TaskDriveReport {
        changed,
        status: supervisor.projection().status(),
        generation: supervisor.projection().generation(),
    })
}

/// Queries independent external evidence after authorization. Unknown remains
/// unknown; this function does not retry execution, claim success or restart a task.
/// # Errors
/// Rejects denied access, substituted evidence and adapter failures.
pub async fn reconcile_effect(
    authorizer: &impl ProcessAuthorizer,
    executor: &impl ExternalEffectExecutor,
    principal: &PrincipalId,
    binding: &tasks::TaskEffectBinding,
) -> Result<ExternalEffectEvidence, PortError> {
    authorize(
        authorizer,
        &binding.request().action.scope(),
        principal,
        ProcessAuthorizationOperation::ReconcileEffect,
    )
    .await?;
    let evidence = executor.reconcile(binding.request()).await?;
    evidence
        .validate_for(binding.request())
        .map_err(|_invalid| PortError::Invariant)?;
    Ok(evidence)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimerDrainReport {
    pub claimed: u16,
    pub acknowledged: u16,
    pub duplicate_inputs: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerDrainStage {
    Authorization,
    Clock,
    Claim,
    Validate,
    Payload,
    Inbox,
    Acknowledge,
}
#[derive(Debug, thiserror::Error)]
#[error("timer delivery failed at {stage:?} after {report:?}: {source}")]
pub struct TimerDrainError {
    pub stage: TimerDrainStage,
    pub report: TimerDrainReport,
    pub source: PortError,
}

/// Stable timer inbox input and durable payload. Delivery attempt, lease token
/// and current time are deliberately excluded so redelivery is immutable.
fn timer_input(lease: &TimerLease) -> Result<(ProcessInput, Vec<u8>), PortError> {
    let bytes = lease
        .timer
        .canonical_wire_bytes()
        .map_err(|_invalid| PortError::Invariant)?;
    let mut key = b"penelope.timer-input.v1\0".to_vec();
    key.extend_from_slice(&lease.timer.action.effect_key().canonical_bytes());
    key.extend_from_slice(lease.timer.action.action_id.as_str().as_bytes());
    let digest = ContentDigest::sha256(&key);
    let mut identity = String::from("inp_timer_");
    for byte in digest.0 {
        write!(identity, "{byte:02x}").map_err(|_format| PortError::Invariant)?;
    }
    let action = &lease.timer.action;
    let input = ProcessInput::new(
        action.tenant_id.clone(),
        action.process_id.clone(),
        identity
            .try_into()
            .map_err(|_invalid| PortError::Invariant)?,
        ProcessInputKind::TimerFired,
        ContentDigest::sha256(&bytes),
    );
    Ok((input, bytes))
}

/// Drains one authorized bounded timer claim into durable inbox acceptance.
/// Persisted payload and a validated inbox receipt precede timer ACK. Acceptance
/// queues input; elapsed time and timer ACK do not prove a saga decision committed.
/// On failure the report identifies partial progress; unacknowledged leases may
/// redeliver safely using the same input identity. The host drives saga decisions.
/// # Errors
/// Returns the exact failed stage and partial report without swallowing failures.
pub async fn drain_timers(
    authorizer: &impl ProcessAuthorizer,
    timers: &impl TimerClaimStore,
    inbox: &(impl Inbox + ProcessPayloadStore),
    request: &TimerClaimRequest,
    clock: &impl Clock,
) -> Result<TimerDrainReport, TimerDrainError> {
    let mut report = TimerDrainReport::default();
    let fail = |stage, partial_report, source| TimerDrainError {
        stage,
        report: partial_report,
        source,
    };
    request
        .validate()
        .map_err(|error| fail(TimerDrainStage::Validate, report, error))?;
    authorize(
        authorizer,
        &request.scope,
        &request.owner,
        ProcessAuthorizationOperation::DeliverTimer,
    )
    .await
    .map_err(|error| fail(TimerDrainStage::Authorization, report, error))?;
    let mut trusted_request = request.clone();
    trusted_request.now = clock
        .now()
        .await
        .map_err(|error| fail(TimerDrainStage::Clock, report, error))?;
    let lease_request = &trusted_request;
    let leases = timers
        .claim_due(lease_request)
        .await
        .map_err(|error| fail(TimerDrainStage::Claim, report, error))?;
    if leases.len() > usize::from(lease_request.limit.get()) {
        return Err(fail(
            TimerDrainStage::Validate,
            report,
            PortError::Invariant,
        ));
    }
    report.claimed = u16::try_from(leases.len())
        .map_err(|_size| fail(TimerDrainStage::Validate, report, PortError::Invariant))?;
    for (index, lease) in leases.iter().enumerate() {
        lease
            .validate_for_claim(lease_request)
            .map_err(|error| fail(TimerDrainStage::Validate, report, error))?;
        if leases.iter().skip(index.saturating_add(1)).any(|other| {
            other.timer.action.action_id == lease.timer.action.action_id
                || other.token == lease.token
        }) {
            return Err(fail(
                TimerDrainStage::Validate,
                report,
                PortError::Invariant,
            ));
        }
    }
    for lease in leases {
        let (input, bytes) =
            timer_input(&lease).map_err(|error| fail(TimerDrainStage::Payload, report, error))?;
        let digest = inbox
            .put_payload(&lease_request.scope, input.payload_digest, &bytes)
            .await
            .map_err(|error| fail(TimerDrainStage::Payload, report, error))?;
        if digest != input.payload_digest {
            return Err(fail(TimerDrainStage::Payload, report, PortError::Invariant));
        }
        let receipt = inbox
            .accept(&input)
            .await
            .map_err(|error| fail(TimerDrainStage::Inbox, report, error))?;
        receipt
            .validate_for(&input)
            .map_err(|_invalid| fail(TimerDrainStage::Inbox, report, PortError::Invariant))?;
        if receipt.duplicate {
            report.duplicate_inputs = report.duplicate_inputs.saturating_add(1);
        }
        let now = clock
            .now()
            .await
            .map_err(|error| fail(TimerDrainStage::Clock, report, error))?;
        timers
            .acknowledge(&lease, now)
            .await
            .map_err(|error| fail(TimerDrainStage::Acknowledge, report, error))?;
        report.acknowledged = report.acknowledged.saturating_add(1);
    }
    Ok(report)
}
