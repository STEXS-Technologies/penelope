//! Compile-time and fail-closed conformance checks for every Penelope port.
//!
//! The library intentionally ships contracts, not infrastructure adapters.
//! These test-only doubles ensure the public trait shapes remain object-safe,
//! `Send + Sync`, callable, and safe when their backing system is unavailable.

#![allow(clippy::panic, clippy::unwrap_used)]

use std::future::Future;
use std::task::{Context, Poll, Waker};

use async_trait::async_trait;
use penelope_domain::{
    ActionId, CanonicalCommand, CausationId, ContentDigest, DefinitionId, DefinitionMigration,
    DefinitionVersion, InputId, LogicalTime, ManualReview, OperationId, OutcomeActor, OutcomeId,
    PrincipalId, ProcessAction, ProcessActionKind, ProcessDefinition, ProcessId, ProcessInput,
    ProcessInputKind, ProcessOutcome, ProcessOutcomeFact, ProcessOutcomeKind, ProcessScope,
    ReviewId, StepId, TenantId,
};
use penelope_ports::{
    ActionDispatchReceipt, ActionDispatcher, ActionIdSource, AtomicProcessCommit,
    AtomicProcessCommitReceipt, CanonicalReconciliation, CanonicalState, CanonicalSubmitReceipt,
    Clock, DefinitionLookup, DefinitionMigrationReceipt, DefinitionRegistrationReceipt,
    DefinitionRegistry, DiagnosticSink, EffectDispatchRequest, ExternalEffectEvidence,
    ExternalEffectExecutor, Inbox, InboxAcceptanceReceipt, LeaseTokenSource, ManualReviewClaim,
    ManualReviewDecision, ManualReviewQueue, ManualReviewReceipt, ManualReviewResolution,
    OutboxClaimRequest, OutboxLease, OutboxRecord, OutboxStore, OutcomeIdSource, PortError,
    ProcessAuthorizationDecision, ProcessAuthorizationRequest, ProcessAuthorizer, ProcessStore,
    RedactedDiagnostic, TimerClaimRequest, TimerClaimStore, TimerLease, TimerSchedule,
    TimerScheduler,
};

struct UnavailablePorts;

#[async_trait]
impl LeaseTokenSource for UnavailablePorts {
    async fn next_lease_token(
        &self,
        _: &ProcessScope,
    ) -> Result<penelope_ports::OutboxLeaseToken, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl DefinitionRegistry for UnavailablePorts {
    async fn register(
        &self,
        _: &ProcessDefinition,
    ) -> Result<DefinitionRegistrationReceipt, PortError> {
        Err(PortError::Unavailable)
    }

    async fn get(&self, _: &DefinitionLookup) -> Result<ProcessDefinition, PortError> {
        Err(PortError::Unavailable)
    }

    async fn register_migration(
        &self,
        _: &DefinitionMigration,
    ) -> Result<DefinitionMigrationReceipt, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ProcessStore for UnavailablePorts {
    async fn commit(
        &self,
        _: &AtomicProcessCommit,
    ) -> Result<AtomicProcessCommitReceipt, PortError> {
        Err(PortError::Unavailable)
    }

    async fn append_outcomes(
        &self,
        _: &ProcessScope,
        _: u64,
        _: &[ProcessOutcome],
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn read_outcomes(
        &self,
        _: &penelope_ports::OutcomeReplayRequest,
    ) -> Result<penelope_ports::OutcomeReplayPage, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl OutboxStore for UnavailablePorts {
    async fn claim(&self, _: &OutboxClaimRequest) -> Result<Vec<OutboxLease>, PortError> {
        Err(PortError::Unavailable)
    }

    async fn acknowledge(&self, _: &OutboxLease, _: LogicalTime) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn renew(
        &self,
        _: &OutboxLease,
        _: LogicalTime,
        _: LogicalTime,
    ) -> Result<OutboxLease, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl Inbox for UnavailablePorts {
    async fn accept(&self, _: &ProcessInput) -> Result<InboxAcceptanceReceipt, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ActionDispatcher for UnavailablePorts {
    async fn dispatch(&self, _: &ProcessAction) -> Result<ActionDispatchReceipt, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ExternalEffectExecutor for UnavailablePorts {
    async fn execute(
        &self,
        _: &EffectDispatchRequest,
    ) -> Result<ExternalEffectEvidence, PortError> {
        Err(PortError::Unavailable)
    }

    async fn reconcile(
        &self,
        _: &EffectDispatchRequest,
    ) -> Result<ExternalEffectEvidence, PortError> {
        Err(PortError::Unavailable)
    }

    async fn cancel(&self, _: &EffectDispatchRequest) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl TimerScheduler for UnavailablePorts {
    async fn schedule(&self, _: &TimerSchedule) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn cancel(&self, _: &TimerSchedule) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl TimerClaimStore for UnavailablePorts {
    async fn claim_due(&self, _: &TimerClaimRequest) -> Result<Vec<TimerLease>, PortError> {
        Err(PortError::Unavailable)
    }

    async fn acknowledge(&self, _: &TimerLease, _: LogicalTime) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl Clock for UnavailablePorts {
    async fn now(&self) -> Result<LogicalTime, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ActionIdSource for UnavailablePorts {
    async fn next_action_id(&self, _: &ProcessScope) -> Result<ActionId, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl OutcomeIdSource for UnavailablePorts {
    async fn next_outcome_id(&self, _: &ProcessScope) -> Result<OutcomeId, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ProcessAuthorizer for UnavailablePorts {
    async fn authorize(
        &self,
        _: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        Ok(ProcessAuthorizationDecision::Denied)
    }
}

#[async_trait]
impl DiagnosticSink for UnavailablePorts {
    async fn record(&self, _: &RedactedDiagnostic) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl CanonicalState for UnavailablePorts {
    async fn submit(&self, _: &CanonicalCommand) -> Result<CanonicalSubmitReceipt, PortError> {
        Err(PortError::Unavailable)
    }

    async fn reconcile(&self, _: &ProcessAction) -> Result<CanonicalReconciliation, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ManualReviewQueue for UnavailablePorts {
    async fn open(&self, _: &ManualReview) -> Result<ManualReviewReceipt, PortError> {
        Err(PortError::Unavailable)
    }

    async fn claim(&self, _: &ManualReviewClaim) -> Result<ManualReviewReceipt, PortError> {
        Err(PortError::Unavailable)
    }

    async fn decide(&self, _: &ManualReviewDecision) -> Result<ManualReviewReceipt, PortError> {
        Err(PortError::Unavailable)
    }
}

fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
    T::try_from(value).ok().unwrap()
}

fn scope() -> ProcessScope {
    ProcessScope::new(
        id::<TenantId>("tnt_game"),
        id::<ProcessId>("prc_trade"),
        id::<DefinitionId>("def_trade"),
        id::<DefinitionVersion>("dfv_one"),
        ContentDigest([1; 32]),
    )
}

fn action() -> ProcessAction {
    ProcessAction::new(
        scope(),
        id::<ActionId>("act_dispatch"),
        id::<StepId>("stp_settle"),
        0,
        ProcessActionKind::CanonicalCommand,
        ContentDigest([2; 32]),
    )
}

fn input() -> ProcessInput {
    ProcessInput::new(
        id::<TenantId>("tnt_game"),
        id::<ProcessId>("prc_trade"),
        id::<InputId>("inp_event"),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([3; 32]),
    )
}

fn outcome() -> ProcessOutcome {
    ProcessOutcome::new(
        scope(),
        0,
        ProcessOutcomeFact::new(
            id::<OutcomeId>("out_started"),
            CausationId::Input(id("inp_event")),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::Started,
            ContentDigest([4; 32]),
        ),
    )
}

fn command() -> CanonicalCommand {
    CanonicalCommand::new(
        id::<TenantId>("tnt_game"),
        id::<ActionId>("act_dispatch"),
        id::<OperationId>("op_settle"),
        vec![id("res_listing")],
        ContentDigest([5; 32]),
    )
    .unwrap()
}

fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the immediate test double unexpectedly blocked"),
    }
}

const fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn every_port_is_object_safe_send_sync_callable_and_fail_closed() {
    assert_send_sync::<UnavailablePorts>();
    let ports = UnavailablePorts;
    let lease_tokens: &dyn LeaseTokenSource = &ports;
    let definitions: &dyn DefinitionRegistry = &ports;
    let process_store: &dyn ProcessStore = &ports;
    let outbox: &dyn OutboxStore = &ports;
    let inbox: &dyn Inbox = &ports;
    let dispatcher: &dyn ActionDispatcher = &ports;
    let external_executor: &dyn ExternalEffectExecutor = &ports;
    let scheduler: &dyn TimerScheduler = &ports;
    let timer_claims: &dyn TimerClaimStore = &ports;
    let clock: &dyn Clock = &ports;
    let action_ids: &dyn ActionIdSource = &ports;
    let outcome_ids: &dyn OutcomeIdSource = &ports;
    let authorizer: &dyn ProcessAuthorizer = &ports;
    let diagnostics: &dyn DiagnosticSink = &ports;
    let canonical: &dyn CanonicalState = &ports;
    let reviews: &dyn ManualReviewQueue = &ports;

    let action = action();
    assert!(matches!(
        ready(lease_tokens.next_lease_token(&scope())),
        Err(PortError::Unavailable)
    ));
    let definition = ProcessDefinition::new(
        id("def_trade"),
        id("dfv_one"),
        ContentDigest([9; 32]),
        vec![id("stp_dispatch")],
    )
    .unwrap();
    let input = input();
    let outcome = outcome();
    let commit = AtomicProcessCommit::new(
        0,
        Some(input.clone()),
        vec![outcome.clone()],
        vec![action.clone()],
    )
    .unwrap();
    let replay_request =
        penelope_ports::OutcomeReplayRequest::new(scope(), 0, std::num::NonZeroU16::MIN).unwrap();
    let review = ManualReview::new(
        scope(),
        id::<ReviewId>("rev_case"),
        0,
        None,
        ContentDigest([6; 32]),
    );
    let request = ProcessAuthorizationRequest {
        scope: scope(),
        principal_id: id::<PrincipalId>("pri_operator"),
        operation: penelope_ports::ProcessAuthorizationOperation::Retry,
    };
    let diagnostic = RedactedDiagnostic::new(
        scope(),
        penelope_ports::DiagnosticClass::Availability,
        ContentDigest([8; 32]),
        0,
    )
    .unwrap();
    let claim = ManualReviewClaim {
        scope: scope(),
        review_id: review.review_id.clone(),
        claimed_by: id::<PrincipalId>("pri_operator"),
        claimed_at: LogicalTime(1),
    };
    let decision = ManualReviewDecision {
        scope: scope(),
        review_id: review.review_id.clone(),
        claimed_by: id::<PrincipalId>("pri_operator"),
        decided_by: id::<PrincipalId>("pri_operator"),
        decided_at: LogicalTime(2),
        resolution: ManualReviewResolution::Escalate,
        control: penelope_ports::ManualReviewControl::SingleOperator,
        evidence_digest: ContentDigest([7; 32]),
    };
    let mut timer_action = action.clone();
    timer_action.kind = ProcessActionKind::Timer;
    let timer = TimerSchedule {
        action: timer_action,
        due_at: LogicalTime(2),
    };
    let effect_request = EffectDispatchRequest::new(action.clone());

    assert!(matches!(
        ready(definitions.register(&definition)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(definitions.get(&DefinitionLookup::new(
            definition.definition_id.clone(),
            definition.definition_version.clone(),
        ))),
        Err(PortError::Unavailable)
    ));
    let migration = DefinitionMigration::new(
        id("mig_trade"),
        definition.definition_id.clone(),
        definition.definition_version.clone(),
        definition.definition_digest,
        id("dfv_two"),
        ContentDigest([8; 32]),
    )
    .unwrap();
    assert!(matches!(
        ready(definitions.register_migration(&migration)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(process_store.commit(&commit)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(process_store.append_outcomes(&scope(), 0, &[outcome])),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(process_store.read_outcomes(&replay_request)),
        Err(PortError::Unavailable)
    ));
    let outbox_claim =
        OutboxClaimRequest::new(scope(), id("pri_worker"), std::num::NonZeroU16::MIN).unwrap();
    let record = OutboxLease {
        record: OutboxRecord::new(action.clone()),
        owner: id("pri_worker"),
        token: penelope_ports::OutboxLeaseToken::new(std::num::NonZeroU64::MIN),
        lease_expires_at: LogicalTime(5),
    };
    assert!(matches!(
        ready(outbox.claim(&outbox_claim)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(outbox.acknowledge(&record, LogicalTime(2))),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(outbox.renew(&record, LogicalTime(2), LogicalTime(6))),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(inbox.accept(&input)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(dispatcher.dispatch(&action)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(external_executor.execute(&effect_request)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(external_executor.reconcile(&effect_request)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(external_executor.cancel(&effect_request)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(scheduler.schedule(&timer)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(scheduler.cancel(&timer)),
        Err(PortError::Unavailable)
    ));
    let timer_claim = TimerClaimRequest::new(
        scope(),
        id("pri_worker"),
        LogicalTime(3),
        std::num::NonZeroU16::MIN,
    )
    .unwrap();
    let timer_lease = TimerLease {
        timer,
        owner: id("pri_worker"),
        token: penelope_ports::OutboxLeaseToken::new(std::num::NonZeroU64::MIN),
        lease_expires_at: LogicalTime(5),
    };
    assert!(matches!(
        ready(timer_claims.claim_due(&timer_claim)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(timer_claims.acknowledge(&timer_lease, LogicalTime(3))),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(ready(clock.now()), Err(PortError::Unavailable)));
    assert!(matches!(
        ready(action_ids.next_action_id(&scope())),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(outcome_ids.next_outcome_id(&scope())),
        Err(PortError::Unavailable)
    ));
    assert_eq!(
        ready(authorizer.authorize(&request)).unwrap(),
        ProcessAuthorizationDecision::Denied
    );
    assert!(matches!(
        ready(diagnostics.record(&diagnostic)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(canonical.submit(&command())),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(canonical.reconcile(&action)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(reviews.open(&review)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(reviews.claim(&claim)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(reviews.decide(&decision)),
        Err(PortError::Unavailable)
    ));
}
