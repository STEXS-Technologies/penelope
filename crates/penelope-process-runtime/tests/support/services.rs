use super::{Stores, id, run, scope};
use async_trait::async_trait;
use penelope_domain::*;
use penelope_executor::services::{TimerDrainStage, claim_outbox, drain_timers};
use penelope_ports::*;
use penelope_sqlite::SqliteProcessStore;
use std::{
    num::NonZeroU16,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

struct Policy {
    allow: AtomicBool,
    requests: Mutex<Vec<ProcessAuthorizationRequest>>,
}
#[async_trait]
impl ProcessAuthorizer for Policy {
    async fn authorize(
        &self,
        request: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(if self.allow.load(Ordering::SeqCst) {
            ProcessAuthorizationDecision::Authorized
        } else {
            ProcessAuthorizationDecision::Denied
        })
    }
}
struct Clock(AtomicU64);
#[async_trait]
impl penelope_ports::Clock for Clock {
    async fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0.load(Ordering::SeqCst)))
    }
}
struct LostAck<'store> {
    store: &'store SqliteProcessStore,
    fail: AtomicBool,
}
#[async_trait]
impl TimerClaimStore for LostAck<'_> {
    async fn claim_due(&self, request: &TimerClaimRequest) -> Result<Vec<TimerLease>, PortError> {
        self.store.claim_due(request).await
    }
    async fn acknowledge(&self, lease: &TimerLease, now: LogicalTime) -> Result<(), PortError> {
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(PortError::Unavailable);
        }
        TimerClaimStore::acknowledge(self.store, lease, now).await
    }
}
fn planned(stores: &Stores, kind: ProcessActionKind) -> AtomicProcessCommit {
    let action = ProcessAction::new(
        stores.scope.clone(),
        id("act_timer"),
        id("stp_retry"),
        0,
        kind,
        ContentDigest::sha256(&[0, 1, 2, 3]),
    );
    let input = ProcessInput::new(
        stores.scope.tenant_id.clone(),
        stores.scope.process_id.clone(),
        id("inp_start"),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([1; 32]),
    );
    let outcome = ProcessOutcome::new(
        stores.scope.clone(),
        0,
        ProcessOutcomeFact::new(
            id("out_start"),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(0),
            ProcessOutcomeKind::Started,
            ContentDigest([2; 32]),
        ),
    );
    let commit = AtomicProcessCommit::new(0, Some(input), vec![outcome], vec![action]).unwrap();
    run(stores.store.commit(&commit)).unwrap();
    commit
}
fn scheduled(stores: &Stores) -> TimerSchedule {
    let commit = planned(stores, ProcessActionKind::Timer);
    let timer = TimerSchedule {
        action: commit.actions.first().unwrap().clone(),
        due_at: LogicalTime(10),
    };
    run(stores.store.schedule(&timer)).unwrap();
    timer
}
#[test]
fn authorized_timer_driver_replays_lost_ack_with_stable_payload_and_input() {
    use penelope_ports::inbox::PendingInbox;
    use penelope_ports::payloads::ProcessPayloadStore;
    let stores = Stores::new(true, scope());
    let timer = scheduled(&stores);
    let policy = Policy {
        allow: AtomicBool::new(false),
        requests: Mutex::new(Vec::new()),
    };
    let request = TimerClaimRequest::new(
        stores.scope.clone(),
        id("pri_timers"),
        LogicalTime(999999),
        NonZeroU16::new(1).unwrap(),
    )
    .unwrap();
    let clock = Clock(AtomicU64::new(10));
    let timers = LostAck {
        store: &stores.store,
        fail: AtomicBool::new(true),
    };
    let denied = run(drain_timers(
        &policy,
        &timers,
        &stores.store,
        &request,
        &clock,
    ))
    .unwrap_err();
    assert_eq!(denied.stage, TimerDrainStage::Authorization);
    assert_eq!(denied.source, PortError::Unauthorized);
    assert_eq!(denied.report.claimed, 0);
    policy.allow.store(true, Ordering::SeqCst);
    let first = run(drain_timers(
        &policy,
        &timers,
        &stores.store,
        &request,
        &clock,
    ))
    .unwrap_err();
    assert_eq!(first.stage, TimerDrainStage::Acknowledge);
    assert_eq!(first.report.claimed, 1);
    assert_eq!(first.report.acknowledged, 0);
    let pending = run(stores.store.pending_inputs(
        &stores.scope.tenant_id,
        &stores.scope.process_id,
        NonZeroU16::new(2).unwrap(),
    ))
    .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending.first().unwrap().kind, ProcessInputKind::TimerFired);
    // The caller's forged future `request.now` was replaced by the trusted clock.
    let held = run(drain_timers(
        &policy,
        &timers,
        &stores.store,
        &request,
        &clock,
    ))
    .unwrap();
    assert_eq!(held.claimed, 0);
    clock.0.store(111, Ordering::SeqCst);
    let recovered = run(drain_timers(
        &policy,
        &timers,
        &stores.store,
        &request,
        &clock,
    ))
    .unwrap();
    assert_eq!(recovered.acknowledged, 1);
    assert_eq!(recovered.duplicate_inputs, 1);
    // Timer delivery ACK is not saga consumption. The host can rediscover the
    // same durable input after either a lost ACK or successful redelivery.
    assert_eq!(
        run(stores.store.pending_inputs(
            &stores.scope.tenant_id,
            &stores.scope.process_id,
            NonZeroU16::new(2).unwrap(),
        ))
        .unwrap(),
        pending
    );
    let bytes = timer.canonical_wire_bytes().unwrap();
    assert_eq!(
        run(stores
            .store
            .read_payload(&stores.scope, ContentDigest::sha256(&bytes)))
        .unwrap(),
        bytes
    );
    assert_eq!(
        run(drain_timers(
            &policy,
            &timers,
            &stores.store,
            &request,
            &clock
        ))
        .unwrap()
        .claimed,
        0
    );
    for authorization in policy.requests.lock().unwrap().iter() {
        assert_eq!(authorization.scope, stores.scope);
        assert_eq!(authorization.principal_id, id("pri_timers"));
        assert_eq!(
            authorization.operation,
            ProcessAuthorizationOperation::DeliverTimer
        );
    }
}
#[test]
fn denied_dispatch_cannot_claim_work_and_allowed_claim_is_exactly_scoped() {
    let stores = Stores::new(false, scope());
    scheduled(&stores);
    let policy = Policy {
        allow: AtomicBool::new(false),
        requests: Mutex::new(Vec::new()),
    };
    let request = OutboxClaimRequest::new(
        stores.scope.clone(),
        id("pri_dispatch"),
        NonZeroU16::new(1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(claim_outbox(
            &policy,
            &stores.store,
            &request,
            LogicalTime(1)
        )),
        Err(PortError::Unauthorized)
    );
    policy.allow.store(true, Ordering::SeqCst);
    let leases = run(claim_outbox(
        &policy,
        &stores.store,
        &request,
        LogicalTime(1),
    ))
    .unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases.first().unwrap().owner, request.owner);
    assert_eq!(
        policy.requests.lock().unwrap().last().unwrap().operation,
        ProcessAuthorizationOperation::Dispatch
    );
}

#[test]
fn task_driver_authorizes_before_starting_or_polling_worker() {
    use super::{Runtime, Supervisor, config, definition};
    use penelope_core::task::{TaskStatus, TaskTick, runtime::TaskClock};
    use penelope_executor::{composition::commit_task_effect, services::drive_task_once};
    use penelope_ports::tasks::TaskEffectBinding;
    use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
    struct Tick;
    impl TaskClock for Tick {
        fn now(&self) -> TaskTick {
            TaskTick::new(11)
        }
    }
    let stores = Stores::new(true, scope());
    let commit = planned(&stores, ProcessActionKind::ExternalEffect);
    let binding = TaskEffectBinding::new(
        EffectDispatchRequest::new(commit.actions.first().unwrap().clone()),
        definition(2),
    )
    .unwrap();
    let committed = run(commit_task_effect(&stores.store, &commit, binding)).unwrap();
    let claim = OutboxClaimRequest::new(
        stores.scope.clone(),
        id("pri_worker"),
        NonZeroU16::new(1).unwrap(),
    )
    .unwrap();
    let lease = run(stores.store.claim(&claim)).unwrap().pop().unwrap();
    let journal =
        SqliteTaskJournal::open(&stores.task_path, definition(2), SqliteLimits::default()).unwrap();
    let authority =
        SqliteTaskJournal::open(&stores.task_path, definition(2), SqliteLimits::default()).unwrap();
    let runtime = Runtime::new(authority, config(&stores.directory)).unwrap();
    let mut supervisor: Supervisor = run(committed.accept(
        &stores.store,
        &claim,
        &lease,
        LogicalTime(1),
        journal,
        runtime,
        &[0, 1, 2, 3],
        1,
        TaskTick::new(10),
    ))
    .unwrap();
    let policy = Policy {
        allow: AtomicBool::new(false),
        requests: Mutex::new(Vec::new()),
    };
    let before = *supervisor.projection();
    assert!(
        run(drive_task_once(
            &policy,
            &claim.owner,
            &committed,
            &mut supervisor,
            &Tick
        ))
        .is_err()
    );
    assert_eq!(*supervisor.projection(), before);
    policy.allow.store(true, Ordering::SeqCst);
    let started = run(drive_task_once(
        &policy,
        &claim.owner,
        &committed,
        &mut supervisor,
        &Tick,
    ))
    .unwrap();
    assert!(started.changed);
    assert_eq!(started.status, TaskStatus::Running);
    for _ in 0..100 {
        let report = run(drive_task_once(
            &policy,
            &claim.owner,
            &committed,
            &mut supervisor,
            &Tick,
        ))
        .unwrap();
        if report.status == TaskStatus::Completed {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
    for authorization in policy.requests.lock().unwrap().iter() {
        assert_eq!(authorization.scope, stores.scope);
        assert_eq!(authorization.principal_id, claim.owner);
        assert_eq!(
            authorization.operation,
            ProcessAuthorizationOperation::DriveTask
        );
    }
}

#[test]
fn reconciliation_preserves_unknown_and_never_executes_an_effect() {
    use penelope_executor::services::reconcile_effect;
    use penelope_ports::tasks::TaskEffectBinding;
    struct Remote {
        reads: AtomicU64,
    }
    #[async_trait]
    impl ExternalEffectExecutor for Remote {
        async fn execute(
            &self,
            _request: &EffectDispatchRequest,
        ) -> Result<ExternalEffectEvidence, PortError> {
            panic!("reconciliation must not execute");
        }
        async fn cancel(&self, _request: &EffectDispatchRequest) -> Result<(), PortError> {
            panic!("reconciliation must not cancel");
        }
        async fn reconcile(
            &self,
            request: &EffectDispatchRequest,
        ) -> Result<ExternalEffectEvidence, PortError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(ExternalEffectEvidence {
                action_id: request.action.action_id.clone(),
                effect_key: request.effect_key.clone(),
                external_reference_id: None,
                state: ExternalEffectState::Unknown,
            })
        }
    }
    let stores = Stores::new(true, scope());
    let commit = planned(&stores, ProcessActionKind::ExternalEffect);
    let binding = TaskEffectBinding::new(
        EffectDispatchRequest::new(commit.actions.first().unwrap().clone()),
        super::definition(2),
    )
    .unwrap();
    let policy = Policy {
        allow: AtomicBool::new(false),
        requests: Mutex::new(Vec::new()),
    };
    let remote = Remote {
        reads: AtomicU64::new(0),
    };
    assert_eq!(
        run(reconcile_effect(
            &policy,
            &remote,
            &id("pri_reconcile"),
            &binding
        )),
        Err(PortError::Unauthorized)
    );
    assert_eq!(remote.reads.load(Ordering::SeqCst), 0);
    policy.allow.store(true, Ordering::SeqCst);
    assert_eq!(
        run(reconcile_effect(
            &policy,
            &remote,
            &id("pri_reconcile"),
            &binding
        ))
        .unwrap()
        .state,
        ExternalEffectState::Unknown
    );
    assert_eq!(remote.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn outbox_driver_rejects_forged_receipt_and_recovers_after_expired_ack() {
    use penelope_executor::services::{DispatchStage, dispatch_outbox};
    use penelope_sqlite::SqliteLimits;
    use std::sync::Arc;
    struct Scheduler<'ports> {
        store: &'ports SqliteProcessStore,
        clock: Arc<Clock>,
        calls: AtomicU64,
        bad_receipt: AtomicBool,
        delay: AtomicBool,
    }
    #[async_trait]
    impl ActionDispatcher for Scheduler<'_> {
        async fn dispatch(
            &self,
            action: &ProcessAction,
        ) -> Result<ActionDispatchReceipt, PortError> {
            self.store
                .schedule(&TimerSchedule {
                    action: action.clone(),
                    due_at: LogicalTime(10),
                })
                .await?;
            let previous = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.bad_receipt.swap(false, Ordering::SeqCst) {
                return Ok(ActionDispatchReceipt::new(
                    action.scope(),
                    id("act_forged"),
                    false,
                ));
            }
            if self.delay.swap(false, Ordering::SeqCst) {
                self.clock.0.fetch_add(101, Ordering::SeqCst);
            }
            Ok(ActionDispatchReceipt::new(
                action.scope(),
                action.action_id.clone(),
                previous > 0,
            ))
        }
    }
    let stores = Stores::new(true, scope());
    planned(&stores, ProcessActionKind::Timer);
    let clock = Arc::new(Clock(AtomicU64::new(1)));
    let outbox = SqliteProcessStore::open(
        &stores.process_path,
        SqliteLimits::default(),
        clock.clone(),
        100,
        8,
    )
    .unwrap();
    let dispatcher = Scheduler {
        store: &stores.store,
        clock: clock.clone(),
        calls: AtomicU64::new(0),
        bad_receipt: AtomicBool::new(true),
        delay: AtomicBool::new(true),
    };
    let policy = Policy {
        allow: AtomicBool::new(true),
        requests: Mutex::new(Vec::new()),
    };
    let request = OutboxClaimRequest::new(
        stores.scope.clone(),
        id("pri_dispatch"),
        NonZeroU16::new(1).unwrap(),
    )
    .unwrap();
    let forged = run(dispatch_outbox(
        &policy,
        &outbox,
        &dispatcher,
        &request,
        clock.as_ref(),
    ))
    .unwrap_err();
    assert_eq!(forged.stage, DispatchStage::Receipt);
    assert_eq!(forged.report.acknowledged, 0);
    assert_eq!(forged.report.delivered, 0);
    clock.0.store(111, Ordering::SeqCst);
    let expired = run(dispatch_outbox(
        &policy,
        &outbox,
        &dispatcher,
        &request,
        clock.as_ref(),
    ))
    .unwrap_err();
    assert_eq!(expired.stage, DispatchStage::Acknowledge);
    assert_eq!(expired.source, PortError::TimedOut);
    assert_eq!(expired.report.delivered, 1);
    assert_eq!(expired.report.duplicate_deliveries, 1);
    assert_eq!(expired.report.acknowledged, 0);
    let recovered = run(dispatch_outbox(
        &policy,
        &outbox,
        &dispatcher,
        &request,
        clock.as_ref(),
    ))
    .unwrap();
    assert_eq!(recovered.acknowledged, 1);
    assert_eq!(recovered.duplicate_deliveries, 1);
    assert_eq!(
        run(dispatch_outbox(
            &policy,
            &outbox,
            &dispatcher,
            &request,
            clock.as_ref()
        ))
        .unwrap()
        .claimed,
        0
    );
    let due = TimerClaimRequest::new(
        stores.scope.clone(),
        id("pri_timers"),
        LogicalTime(212),
        NonZeroU16::new(2).unwrap(),
    )
    .unwrap();
    assert_eq!(run(stores.store.claim_due(&due)).unwrap().len(), 1);
}

impl penelope_sqlite::SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0.load(Ordering::SeqCst)))
    }
}

#[path = "service_faults.rs"]
mod service_faults;
