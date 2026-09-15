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
    ActionId, CanonicalCommandDtoV1, CausationIdV1, ContentDigest, DefinitionId,
    DefinitionMigrationV1, DefinitionVersion, InputId, LogicalTimeV1, ManualReviewDtoV1,
    OperationId, OutcomeActorV1, OutcomeId, PrincipalId, ProcessActionDtoV1, ProcessActionKindV1,
    ProcessDefinitionDtoV1, ProcessId, ProcessInputDtoV1, ProcessInputKindV1, ProcessOutcomeDtoV1,
    ProcessOutcomeFactV1, ProcessOutcomeKindV1, ProcessScopeV1, ReviewId, StepId, TenantId,
};
use penelope_ports::{
    ActionDispatchReceiptV1, ActionDispatcher, ActionIdSource, AtomicProcessCommitReceiptV1,
    AtomicProcessCommitV1, CanonicalReconciliationV1, CanonicalState, Clock, DefinitionLookupV1,
    DefinitionRegistry, EffectDispatchRequestV1, ExternalEffectEvidenceV1, ExternalEffectExecutor,
    Inbox, InboxAcceptanceReceiptV1, LeaseTokenSource, ManualReviewClaimV1, ManualReviewDecisionV1,
    ManualReviewQueue, ManualReviewReceiptV1, ManualReviewResolutionV1, OutboxClaimRequestV1,
    OutboxLeaseV1, OutboxRecordV1, OutboxStore, OutcomeIdSource, PortError,
    ProcessAuthorizationDecisionV1, ProcessAuthorizationRequestV1, ProcessAuthorizer, ProcessStore,
    TimerClaimRequestV1, TimerClaimStore, TimerLeaseV1, TimerScheduleV1, TimerScheduler,
};

struct UnavailablePorts;

#[async_trait]
impl LeaseTokenSource for UnavailablePorts {
    async fn next_lease_token(
        &self,
        _: &ProcessScopeV1,
    ) -> Result<penelope_ports::OutboxLeaseTokenV1, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl DefinitionRegistry for UnavailablePorts {
    async fn register(&self, _: &ProcessDefinitionDtoV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn get(&self, _: &DefinitionLookupV1) -> Result<ProcessDefinitionDtoV1, PortError> {
        Err(PortError::Unavailable)
    }

    async fn register_migration(&self, _: &DefinitionMigrationV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ProcessStore for UnavailablePorts {
    async fn commit(
        &self,
        _: &AtomicProcessCommitV1,
    ) -> Result<AtomicProcessCommitReceiptV1, PortError> {
        Err(PortError::Unavailable)
    }

    async fn append_outcomes(
        &self,
        _: &ProcessScopeV1,
        _: u64,
        _: &[ProcessOutcomeDtoV1],
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn read_outcomes(
        &self,
        _: &penelope_ports::OutcomeReplayRequestV1,
    ) -> Result<penelope_ports::OutcomeReplayPageV1, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl OutboxStore for UnavailablePorts {
    async fn claim(&self, _: &OutboxClaimRequestV1) -> Result<Vec<OutboxLeaseV1>, PortError> {
        Err(PortError::Unavailable)
    }

    async fn acknowledge(&self, _: &OutboxLeaseV1, _: LogicalTimeV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn renew(
        &self,
        _: &OutboxLeaseV1,
        _: LogicalTimeV1,
        _: LogicalTimeV1,
    ) -> Result<OutboxLeaseV1, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl Inbox for UnavailablePorts {
    async fn accept(&self, _: &ProcessInputDtoV1) -> Result<InboxAcceptanceReceiptV1, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ActionDispatcher for UnavailablePorts {
    async fn dispatch(&self, _: &ProcessActionDtoV1) -> Result<ActionDispatchReceiptV1, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ExternalEffectExecutor for UnavailablePorts {
    async fn execute(
        &self,
        _: &EffectDispatchRequestV1,
    ) -> Result<ExternalEffectEvidenceV1, PortError> {
        Err(PortError::Unavailable)
    }

    async fn reconcile(
        &self,
        _: &EffectDispatchRequestV1,
    ) -> Result<ExternalEffectEvidenceV1, PortError> {
        Err(PortError::Unavailable)
    }

    async fn cancel(&self, _: &EffectDispatchRequestV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl TimerScheduler for UnavailablePorts {
    async fn schedule(&self, _: &TimerScheduleV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn cancel(&self, _: &TimerScheduleV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl TimerClaimStore for UnavailablePorts {
    async fn claim_due(&self, _: &TimerClaimRequestV1) -> Result<Vec<TimerLeaseV1>, PortError> {
        Err(PortError::Unavailable)
    }

    async fn acknowledge(&self, _: &TimerLeaseV1, _: LogicalTimeV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl Clock for UnavailablePorts {
    async fn now(&self) -> Result<LogicalTimeV1, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ActionIdSource for UnavailablePorts {
    async fn next_action_id(&self, _: &ProcessScopeV1) -> Result<ActionId, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl OutcomeIdSource for UnavailablePorts {
    async fn next_outcome_id(&self, _: &ProcessScopeV1) -> Result<OutcomeId, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ProcessAuthorizer for UnavailablePorts {
    async fn authorize(
        &self,
        _: &ProcessAuthorizationRequestV1,
    ) -> Result<ProcessAuthorizationDecisionV1, PortError> {
        Ok(ProcessAuthorizationDecisionV1::Denied)
    }
}

#[async_trait]
impl CanonicalState for UnavailablePorts {
    async fn submit(&self, _: &CanonicalCommandDtoV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn reconcile(
        &self,
        _: &ProcessActionDtoV1,
    ) -> Result<CanonicalReconciliationV1, PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ManualReviewQueue for UnavailablePorts {
    async fn open(&self, _: &ManualReviewDtoV1) -> Result<ManualReviewReceiptV1, PortError> {
        Err(PortError::Unavailable)
    }

    async fn claim(&self, _: &ManualReviewClaimV1) -> Result<ManualReviewReceiptV1, PortError> {
        Err(PortError::Unavailable)
    }

    async fn decide(&self, _: &ManualReviewDecisionV1) -> Result<ManualReviewReceiptV1, PortError> {
        Err(PortError::Unavailable)
    }
}

fn id<T: TryFrom<&'static str>>(value: &'static str) -> T {
    T::try_from(value).ok().unwrap()
}

fn scope() -> ProcessScopeV1 {
    ProcessScopeV1::new(
        id::<TenantId>("tnt_game"),
        id::<ProcessId>("prc_trade"),
        id::<DefinitionId>("def_trade"),
        id::<DefinitionVersion>("dfv_one"),
        ContentDigest([1; 32]),
    )
}

fn action() -> ProcessActionDtoV1 {
    ProcessActionDtoV1::new(
        scope(),
        id::<ActionId>("act_dispatch"),
        id::<StepId>("stp_settle"),
        0,
        ProcessActionKindV1::CanonicalCommand,
        ContentDigest([2; 32]),
    )
}

fn input() -> ProcessInputDtoV1 {
    ProcessInputDtoV1::new(
        id::<TenantId>("tnt_game"),
        id::<ProcessId>("prc_trade"),
        id::<InputId>("inp_event"),
        ProcessInputKindV1::CanonicalEvent,
        ContentDigest([3; 32]),
    )
}

fn outcome() -> ProcessOutcomeDtoV1 {
    ProcessOutcomeDtoV1::new(
        scope(),
        0,
        ProcessOutcomeFactV1::new(
            id::<OutcomeId>("out_started"),
            CausationIdV1::Input(id("inp_event")),
            OutcomeActorV1::System,
            LogicalTimeV1(1),
            ProcessOutcomeKindV1::Started,
            ContentDigest([4; 32]),
        ),
    )
}

fn command() -> CanonicalCommandDtoV1 {
    CanonicalCommandDtoV1::new(
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
    let canonical: &dyn CanonicalState = &ports;
    let reviews: &dyn ManualReviewQueue = &ports;

    let action = action();
    assert!(matches!(
        ready(lease_tokens.next_lease_token(&scope())),
        Err(PortError::Unavailable)
    ));
    let definition = ProcessDefinitionDtoV1::new(
        id("def_trade"),
        id("dfv_one"),
        ContentDigest([9; 32]),
        vec![id("stp_dispatch")],
    )
    .unwrap();
    let input = input();
    let outcome = outcome();
    let commit = AtomicProcessCommitV1::new(
        0,
        Some(input.clone()),
        vec![outcome.clone()],
        vec![action.clone()],
    )
    .unwrap();
    let replay_request =
        penelope_ports::OutcomeReplayRequestV1::new(scope(), 0, std::num::NonZeroU16::MIN).unwrap();
    let review = ManualReviewDtoV1::new(
        scope(),
        id::<ReviewId>("rev_case"),
        0,
        None,
        ContentDigest([6; 32]),
    );
    let request = ProcessAuthorizationRequestV1 {
        scope: scope(),
        principal_id: id::<PrincipalId>("pri_operator"),
        operation: penelope_ports::ProcessAuthorizationOperationV1::Retry,
    };
    let claim = ManualReviewClaimV1 {
        scope: scope(),
        review_id: review.review_id.clone(),
        claimed_by: id::<PrincipalId>("pri_operator"),
        claimed_at: LogicalTimeV1(1),
    };
    let decision = ManualReviewDecisionV1 {
        scope: scope(),
        review_id: review.review_id.clone(),
        claimed_by: id::<PrincipalId>("pri_operator"),
        decided_by: id::<PrincipalId>("pri_operator"),
        decided_at: LogicalTimeV1(2),
        resolution: ManualReviewResolutionV1::Escalate,
        control: penelope_ports::ManualReviewControlV1::SingleOperator,
        evidence_digest: ContentDigest([7; 32]),
    };
    let mut timer_action = action.clone();
    timer_action.kind = ProcessActionKindV1::Timer;
    let timer = TimerScheduleV1 {
        action: timer_action,
        due_at: LogicalTimeV1(2),
    };
    let effect_request = EffectDispatchRequestV1::new(action.clone());

    assert!(matches!(
        ready(definitions.register(&definition)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(definitions.get(&DefinitionLookupV1::new(
            definition.definition_id.clone(),
            definition.definition_version.clone(),
        ))),
        Err(PortError::Unavailable)
    ));
    let migration = DefinitionMigrationV1::new(
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
        OutboxClaimRequestV1::new(scope(), id("pri_worker"), std::num::NonZeroU16::MIN).unwrap();
    let record = OutboxLeaseV1 {
        record: OutboxRecordV1::new(action.clone()),
        owner: id("pri_worker"),
        token: penelope_ports::OutboxLeaseTokenV1::new(std::num::NonZeroU64::MIN),
        lease_expires_at: LogicalTimeV1(5),
    };
    assert!(matches!(
        ready(outbox.claim(&outbox_claim)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(outbox.acknowledge(&record, LogicalTimeV1(2))),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(outbox.renew(&record, LogicalTimeV1(2), LogicalTimeV1(6))),
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
    let timer_claim = TimerClaimRequestV1::new(
        scope(),
        id("pri_worker"),
        LogicalTimeV1(3),
        std::num::NonZeroU16::MIN,
    )
    .unwrap();
    let timer_lease = TimerLeaseV1 {
        timer,
        owner: id("pri_worker"),
        token: penelope_ports::OutboxLeaseTokenV1::new(std::num::NonZeroU64::MIN),
        lease_expires_at: LogicalTimeV1(5),
    };
    assert!(matches!(
        ready(timer_claims.claim_due(&timer_claim)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(timer_claims.acknowledge(&timer_lease, LogicalTimeV1(3))),
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
        ProcessAuthorizationDecisionV1::Denied
    );
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
