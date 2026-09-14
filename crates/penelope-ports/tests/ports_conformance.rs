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
    ActionId, CanonicalCommandDtoV1, CausationIdV1, ContentDigest, DefinitionId, DefinitionVersion,
    InputId, LogicalTimeV1, ManualReviewDtoV1, OperationId, OutcomeActorV1, OutcomeId, PrincipalId,
    ProcessActionDtoV1, ProcessActionKindV1, ProcessId, ProcessInputDtoV1, ProcessInputKindV1,
    ProcessOutcomeDtoV1, ProcessOutcomeFactV1, ProcessOutcomeKindV1, ProcessScopeV1, ReviewId,
    StepId, TenantId,
};
use penelope_ports::{
    ActionDispatcher, ActionIdSource, AtomicProcessCommitReceiptV1, AtomicProcessCommitV1,
    CanonicalReconciliationV1, CanonicalState, Clock, Inbox, ManualReviewClaimV1,
    ManualReviewDecisionV1, ManualReviewQueue, ManualReviewResolutionV1, OutcomeIdSource,
    PortError, ProcessAuthorizationDecisionV1, ProcessAuthorizationRequestV1, ProcessAuthorizer,
    ProcessStore, TimerScheduleV1, TimerScheduler,
};

struct UnavailablePorts;

#[async_trait]
impl ProcessStore for UnavailablePorts {
    async fn commit(
        &self,
        _: &AtomicProcessCommitV1,
    ) -> Result<AtomicProcessCommitReceiptV1, PortError> {
        Err(PortError::Unavailable)
    }

    async fn append_outcomes(&self, _: u64, _: &[ProcessOutcomeDtoV1]) -> Result<(), PortError> {
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
impl Inbox for UnavailablePorts {
    async fn accept(&self, _: &ProcessInputDtoV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl ActionDispatcher for UnavailablePorts {
    async fn dispatch(&self, _: &ProcessActionDtoV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}

#[async_trait]
impl TimerScheduler for UnavailablePorts {
    async fn schedule(&self, _: &TimerScheduleV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn cancel(&self, _: &ActionId) -> Result<(), PortError> {
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
    async fn open(&self, _: &ManualReviewDtoV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn claim(&self, _: &ManualReviewClaimV1) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }

    async fn decide(&self, _: &ManualReviewDecisionV1) -> Result<(), PortError> {
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
    let process_store: &dyn ProcessStore = &ports;
    let inbox: &dyn Inbox = &ports;
    let dispatcher: &dyn ActionDispatcher = &ports;
    let scheduler: &dyn TimerScheduler = &ports;
    let clock: &dyn Clock = &ports;
    let action_ids: &dyn ActionIdSource = &ports;
    let outcome_ids: &dyn OutcomeIdSource = &ports;
    let authorizer: &dyn ProcessAuthorizer = &ports;
    let canonical: &dyn CanonicalState = &ports;
    let reviews: &dyn ManualReviewQueue = &ports;

    let action = action();
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
    };
    let decision = ManualReviewDecisionV1 {
        scope: scope(),
        review_id: review.review_id.clone(),
        claimed_by: id::<PrincipalId>("pri_operator"),
        decided_by: id::<PrincipalId>("pri_operator"),
        resolution: ManualReviewResolutionV1::Escalate,
        control: penelope_ports::ManualReviewControlV1::SingleOperator,
        evidence_digest: ContentDigest([7; 32]),
    };
    let timer = TimerScheduleV1 {
        action: action.clone(),
        due_at: LogicalTimeV1(2),
    };

    assert!(matches!(
        ready(process_store.commit(&commit)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(process_store.append_outcomes(0, &[outcome])),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(process_store.read_outcomes(&replay_request)),
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
        ready(scheduler.schedule(&timer)),
        Err(PortError::Unavailable)
    ));
    assert!(matches!(
        ready(scheduler.cancel(&action.action_id)),
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
