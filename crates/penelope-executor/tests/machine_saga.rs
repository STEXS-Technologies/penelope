//! Native linear/graph admission on real source and receiver stores.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use async_trait::async_trait;
use penelope_core::machine::*;
use penelope_domain::*;
use penelope_executor::{engine::*, graph::*, machine_saga::*};
use penelope_ports::{machine::*, payloads::ProcessPayloadStore, *};
use penelope_sqlite::{
    SqliteLeaseClock, SqliteLimits, SqliteMachineOutbox, SqliteMachineStore, SqliteProcessStore,
};
use std::{
    num::{NonZeroU16, NonZeroU32},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};
#[path = "../../penelope-sqlite/tests/support/machines.rs"]
#[allow(dead_code)]
mod machine;
use machine::{SCOPE, block_on};
const NAMESPACE: MachineSourceId = MachineSourceId::from_bytes([7; 32]);
const LIMITS: MachineOutboxLimits = MachineOutboxLimits {
    lease_ticks: 10,
    max_claims: 1,
    max_claim_bytes: 512,
    max_attempts: 4,
};
struct Clock(AtomicU64);
impl MachineLeaseClock for Clock {
    fn now(&self) -> Result<MachineLeaseTick, MachineStoreError> {
        Ok(MachineLeaseTick::new(self.0.load(Ordering::SeqCst)))
    }
}
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0.load(Ordering::SeqCst)))
    }
}
struct Policy {
    allowed: AtomicBool,
    calls: AtomicUsize,
}
impl Policy {
    fn new(allowed: bool) -> Arc<Self> {
        Arc::new(Self {
            allowed: AtomicBool::new(allowed),
            calls: AtomicUsize::new(0),
        })
    }
}
#[async_trait]
impl ProcessAuthorizer for Policy {
    async fn authorize(
        &self,
        request: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.principal_id, id("pri_receiver"));
        assert_eq!(request.operation, ProcessAuthorizationOperation::Start);
        assert_eq!(request.scope.definition_id, id("def_machine"));
        Ok(if self.allowed.load(Ordering::SeqCst) {
            ProcessAuthorizationDecision::Authorized
        } else {
            ProcessAuthorizationDecision::Denied
        })
    }
}
fn id<T: std::str::FromStr>(s: &str) -> T {
    s.parse().ok().unwrap()
}
fn definition(graph: bool) -> MachineSagaDefinition {
    let steps = vec![StepPlan {
        step_id: id("stp_work"),
        action_kind: ProcessActionKind::ExternalEffect,
        payload_digest: ContentDigest([2; 32]),
        retry_policy: RetryPolicy::no_retry(),
        compensation: None,
    }];
    if graph {
        MachineSagaDefinition::Graph(ProcessGraphDefinition {
            definition_id: id("def_machine"),
            definition_version: id("dfv_one"),
            definition_digest: ContentDigest([3; 32]),
            steps,
            entry_step_id: id("stp_work"),
            transitions: vec![GraphTransition {
                from_step: id("stp_work"),
                on: GraphTransitionOutcome::Succeeded,
                to_step: None,
            }],
            max_step_visits: NonZeroU32::MIN,
        })
    } else {
        MachineSagaDefinition::Linear(LinearSagaDefinition {
            definition_id: id("def_machine"),
            definition_version: id("dfv_one"),
            definition_digest: ContentDigest([3; 32]),
            steps,
        })
    }
}
fn source(path: &std::path::Path, amount: u64) -> Arc<SqliteMachineStore<machine::Policy, 2>> {
    let store = Arc::new(machine::open(path));
    block_on(store.bootstrap(&true, machine::genesis())).unwrap();
    block_on(store.commit(&true, machine::batch(&mut machine::owner(), amount, 4))).unwrap();
    store
}
fn process(path: &std::path::Path) -> Arc<SqliteProcessStore> {
    Arc::new(
        SqliteProcessStore::open(
            path,
            SqliteLimits::default(),
            Arc::new(Clock(AtomicU64::new(100))),
            10,
            8,
        )
        .unwrap(),
    )
}
const fn claim() -> MachineIntentClaim {
    MachineIntentClaim {
        scope: SCOPE,
        owner: MachineLeaseOwner::from_bytes([9; 32]),
        max_intents: 1,
        max_bytes: 512,
    }
}
fn make_receiver<S: ProcessStore + ProcessPayloadStore + ProcessCommitVerifier>(
    store: Arc<S>,
    policy: Arc<Policy>,
    graph: bool,
) -> Arc<MachineSagaReceiver<S, Policy>> {
    Arc::new(
        MachineSagaReceiver::new(
            store,
            policy,
            NAMESPACE,
            id("pri_receiver"),
            definition(graph),
            64,
        )
        .unwrap(),
    )
}
#[test]
fn linear_and_graph_bootstrap_replay_and_separate_ack_survive_reopen_in_both_layouts() {
    for graph in [false, true] {
        for shared in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let source_path = directory.path().join("source.db");
            let native_path = if shared {
                source_path.clone()
            } else {
                directory.path().join("native.db")
            };
            let native = process(&native_path);
            let policy = Policy::new(true);
            let receiver = make_receiver(Arc::clone(&native), Arc::clone(&policy), graph);
            let clock = Arc::new(Clock(AtomicU64::new(100)));
            let outbox = SqliteMachineOutbox::new(
                source(&source_path, 8),
                Arc::clone(&clock),
                Arc::clone(&receiver),
                LIMITS,
            )
            .unwrap();
            let first = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
            let admitted =
                block_on(receiver.accept(&outbox, &true, &first, LogicalTime(1))).unwrap();
            assert!(admitted.commit().input.is_none());
            assert!(admitted.commit().canonical_source_event_id.is_none());
            assert_eq!(admitted.commit().outcomes.len(), 3);
            assert_eq!(admitted.commit().actions.len(), 1);
            let scope = receiver.scope(first.metadata().source).unwrap();
            let event =
                block_on(native.read_payload(&scope, admitted.commit().outcomes[0].payload_digest))
                    .unwrap();
            match definition(graph) {
                MachineSagaDefinition::Linear(d) => {
                    let event = serde_json::from_slice::<LinearSagaEvent>(&event).unwrap();
                    let replayed =
                        replay(&d, &scope.tenant_id, &scope.process_id, &[event]).unwrap();
                    assert_eq!(
                        replayed.next_action.as_ref(),
                        admitted.commit().actions.first()
                    );
                }
                MachineSagaDefinition::Graph(d) => {
                    let event = serde_json::from_slice::<GraphSagaEvent>(&event).unwrap();
                    assert_eq!(
                        replay_graph(&d, &scope.tenant_id, &scope.process_id, &[event])
                            .unwrap()
                            .active_action_id,
                        Some(admitted.commit().actions[0].action_id.clone())
                    );
                }
            }
            assert!(
                block_on(outbox.status(&true, SCOPE, first.metadata().source.intent.identity))
                    .unwrap()
                    .unwrap()
                    .accepted
                    .is_none()
            );
            drop(outbox);
            drop(receiver);
            drop(native);
            clock.0.store(110, Ordering::SeqCst);
            let reopened_native = process(&native_path);
            let reopened_receiver = make_receiver(Arc::clone(&reopened_native), policy, graph);
            let store = Arc::new(machine::open(&source_path));
            let reopened_outbox =
                SqliteMachineOutbox::new(store, clock, Arc::clone(&reopened_receiver), LIMITS)
                    .unwrap();
            let second = block_on(reopened_outbox.claim(&true, claim()))
                .unwrap()
                .remove(0);
            assert!(second.requires_reconciliation());
            let reconstructed = block_on(reopened_receiver.accept(
                &reopened_outbox,
                &true,
                &second,
                LogicalTime(999),
            ))
            .unwrap();
            assert_eq!(reconstructed, admitted);
            assert!(block_on(reopened_outbox.acknowledge(&true, &first, &admitted)).is_err());
            let accepted =
                block_on(reopened_outbox.acknowledge(&true, &second, &reconstructed)).unwrap();
            assert_eq!(
                block_on(reopened_outbox.acknowledge(&true, &second, &reconstructed)).unwrap(),
                accepted
            );
            let actions = block_on(reopened_native.claim(
                &OutboxClaimRequest::new(scope.clone(), id("pri_worker"), NonZeroU16::MIN).unwrap(),
            ))
            .unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].record.action, admitted.commit().actions[0]);
            let page = block_on(reopened_native.read_outcomes(
                &OutcomeReplayRequest::new(scope, 0, NonZeroU16::new(32).unwrap()).unwrap(),
            ))
            .unwrap();
            assert_eq!(page.outcomes, admitted.commit().outcomes);
        }
    }
}
#[test]
fn source_and_native_authorization_precede_all_native_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native.db");
    let native = process(&path);
    let policy = Policy::new(false);
    let receiver = make_receiver(native, Arc::clone(&policy), false);
    let outbox = SqliteMachineOutbox::new(
        source(&directory.path().join("source.db"), 8),
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        LIMITS,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    assert!(matches!(
        block_on(receiver.accept(&outbox, &false, &lease, LogicalTime(1))),
        Err(MachineSagaHandoffError::Source(MachineStoreError::Denied))
    ));
    assert_eq!(policy.calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(1))),
        Err(MachineSagaHandoffError::Port(PortError::Unauthorized))
    ));
    let sql = rusqlite::Connection::open(&path).unwrap();
    for table in ["process_payloads", "process_outcomes", "process_actions"] {
        let count: i64 = sql
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
#[test]
fn changed_receiver_policy_and_foreign_native_receipt_cannot_acknowledge_source() {
    for graph in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let native = process(&directory.path().join("native.db"));
        let receiver = make_receiver(Arc::clone(&native), Policy::new(true), graph);
        let clock = Arc::new(Clock(AtomicU64::new(100)));
        let first = SqliteMachineOutbox::new(
            source(&directory.path().join("first.db"), 8),
            Arc::clone(&clock),
            Arc::clone(&receiver),
            LIMITS,
        )
        .unwrap();
        let second = SqliteMachineOutbox::new(
            source(&directory.path().join("second.db"), 9),
            Arc::clone(&clock),
            Arc::clone(&receiver),
            LIMITS,
        )
        .unwrap();
        let lease = block_on(first.claim(&true, claim())).unwrap().remove(0);
        let foreign = block_on(second.claim(&true, claim())).unwrap().remove(0);
        let receipt = block_on(receiver.accept(&second, &true, &foreign, LogicalTime(1))).unwrap();
        assert!(block_on(first.acknowledge(&true, &lease, &receipt)).is_err());
        let wrong = MachineSagaReceiver::new(
            Arc::clone(&native),
            Policy::new(true),
            MachineSourceId::from_bytes([8; 32]),
            id("pri_receiver"),
            definition(graph),
            64,
        )
        .unwrap();
        assert!(matches!(
            block_on(wrong.accept(&first, &true, &lease, LogicalTime(1))),
            Err(MachineSagaHandoffError::Source(MachineStoreError::Conflict))
        ));
        let correct = block_on(receiver.accept(&first, &true, &lease, LogicalTime(2))).unwrap();
        assert_ne!(
            correct.commit().outcomes[0].process_id,
            receipt.commit().outcomes[0].process_id
        );
        block_on(first.acknowledge(&true, &lease, &correct)).unwrap();
    }
}
#[test]
fn corrupt_native_payload_refuses_acknowledgement_without_reexecuting_a_saga() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("native.db");
    let native = process(&path);
    let receiver = make_receiver(native, Policy::new(true), false);
    let outbox = SqliteMachineOutbox::new(
        source(&directory.path().join("source.db"), 8),
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        LIMITS,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    let receipt = block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(1))).unwrap();
    let sql = rusqlite::Connection::open(path).unwrap();
    sql.execute(
        "UPDATE process_payloads SET bytes=zeroblob(length(bytes))",
        [],
    )
    .unwrap();
    assert!(block_on(outbox.acknowledge(&true, &lease, &receipt)).is_err());
    assert!(block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(2))).is_err());
    let count: i64 = sql
        .query_row("SELECT count(*) FROM process_outcomes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);
}

struct NativeFaults {
    inner: Arc<SqliteProcessStore>,
    lose_ack: AtomicBool,
    barrier: Option<Arc<std::sync::Barrier>>,
}
#[async_trait]
impl ProcessStore for NativeFaults {
    async fn commit(
        &self,
        commit: &AtomicProcessCommit,
    ) -> Result<AtomicProcessCommitReceipt, PortError> {
        if let Some(barrier) = &self.barrier {
            barrier.wait();
        }
        #[cfg(unix)]
        crash::cut("before-native-commit");
        let receipt = self.inner.commit(commit).await?;
        #[cfg(unix)]
        crash::cut("after-native-commit");
        if self.lose_ack.swap(false, Ordering::SeqCst) {
            Err(PortError::Unavailable)
        } else {
            Ok(receipt)
        }
    }
    async fn append_outcomes(
        &self,
        scope: &ProcessScope,
        sequence: u64,
        outcomes: &[ProcessOutcome],
    ) -> Result<(), PortError> {
        self.inner.append_outcomes(scope, sequence, outcomes).await
    }
    async fn read_outcomes(
        &self,
        request: &OutcomeReplayRequest,
    ) -> Result<OutcomeReplayPage, PortError> {
        self.inner.read_outcomes(request).await
    }
}
#[async_trait]
impl ProcessPayloadStore for NativeFaults {
    async fn put_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
        bytes: &[u8],
    ) -> Result<ContentDigest, PortError> {
        self.inner.put_payload(scope, digest, bytes).await
    }
    async fn read_payload(
        &self,
        scope: &ProcessScope,
        digest: ContentDigest,
    ) -> Result<Vec<u8>, PortError> {
        self.inner.read_payload(scope, digest).await
    }
}
#[test]
fn lost_native_ack_is_resolved_by_lookup_and_never_another_process() {
    for graph in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let native = Arc::new(NativeFaults {
            inner: process(&directory.path().join("native.db")),
            lose_ack: AtomicBool::new(true),
            barrier: None,
        });
        let receiver = make_receiver(Arc::clone(&native), Policy::new(true), graph);
        let outbox = SqliteMachineOutbox::new(
            source(&directory.path().join("source.db"), 8),
            Arc::new(Clock(AtomicU64::new(100))),
            Arc::clone(&receiver),
            LIMITS,
        )
        .unwrap();
        let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
        assert!(matches!(
            block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(1))),
            Err(MachineSagaHandoffError::Port(PortError::Unavailable))
        ));
        assert!(
            block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
                .unwrap()
                .unwrap()
                .accepted
                .is_none()
        );
        let receipt = block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(2))).unwrap();
        assert_eq!(receipt.commit().outcomes[0].occurred_at, LogicalTime(1));
        block_on(outbox.acknowledge(&true, &lease, &receipt)).unwrap();
        assert!(!native.lose_ack.load(Ordering::SeqCst));
    }
}
#[test]
fn competing_native_writers_choose_one_original_start_and_action() {
    for graph in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native.db");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let first = make_receiver(
            Arc::new(NativeFaults {
                inner: process(&path),
                lose_ack: AtomicBool::new(false),
                barrier: Some(Arc::clone(&barrier)),
            }),
            Policy::new(true),
            graph,
        );
        let second = make_receiver(
            Arc::new(NativeFaults {
                inner: process(&path),
                lose_ack: AtomicBool::new(false),
                barrier: Some(barrier),
            }),
            Policy::new(true),
            graph,
        );
        let outbox = Arc::new(
            SqliteMachineOutbox::new(
                source(&directory.path().join("source.db"), 8),
                Arc::new(Clock(AtomicU64::new(100))),
                Arc::clone(&first),
                LIMITS,
            )
            .unwrap(),
        );
        let lease = Arc::new(block_on(outbox.claim(&true, claim())).unwrap().remove(0));
        let results = std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                block_on(first.accept(outbox.as_ref(), &true, &lease, LogicalTime(1))).unwrap()
            });
            let two = scope.spawn(|| {
                block_on(second.accept(outbox.as_ref(), &true, &lease, LogicalTime(2))).unwrap()
            });
            (one.join().unwrap(), two.join().unwrap())
        });
        assert_eq!(results.0, results.1);
        block_on(outbox.acknowledge(&true, &lease, &results.0)).unwrap();
        let sql = rusqlite::Connection::open(&path).unwrap();
        let count: i64 = sql
            .query_row("SELECT count(*) FROM process_outcomes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
        let action_count: i64 = sql
            .query_row("SELECT count(*) FROM process_actions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(action_count, 1);
    }
}
#[test]
fn advanced_native_history_is_never_reset_by_source_redelivery() {
    for graph in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let native = process(&directory.path().join("native.db"));
        let receiver = make_receiver(Arc::clone(&native), Policy::new(true), graph);
        let clock = Arc::new(Clock(AtomicU64::new(100)));
        let outbox = SqliteMachineOutbox::new(
            source(&directory.path().join("source.db"), 8),
            Arc::clone(&clock),
            Arc::clone(&receiver),
            LIMITS,
        )
        .unwrap();
        let first = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
        let receipt = block_on(receiver.accept(&outbox, &true, &first, LogicalTime(1))).unwrap();
        let scope = receiver.scope(first.metadata().source).unwrap();
        let outcomes = escalate(&native, &scope, &receipt, graph);
        clock.0.store(110, Ordering::SeqCst);
        let second = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
        assert_eq!(
            block_on(receiver.accept(&outbox, &true, &second, LogicalTime(999))).unwrap(),
            receipt
        );
        block_on(outbox.acknowledge(&true, &second, &receipt)).unwrap();
        let page = block_on(
            native.read_outcomes(&OutcomeReplayRequest::new(scope, 3, NonZeroU16::MIN).unwrap()),
        )
        .unwrap();
        assert_eq!(page.outcomes, outcomes[..1]);
    }
}
#[test]
fn revoked_native_permission_and_expired_source_cannot_acknowledge_admission() {
    let directory = tempfile::tempdir().unwrap();
    let policy = Policy::new(true);
    let receiver = make_receiver(
        process(&directory.path().join("native.db")),
        Arc::clone(&policy),
        false,
    );
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let outbox = SqliteMachineOutbox::new(
        source(&directory.path().join("source.db"), 8),
        Arc::clone(&clock),
        Arc::clone(&receiver),
        LIMITS,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    let receipt = block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(1))).unwrap();
    policy.allowed.store(false, Ordering::SeqCst);
    assert_eq!(
        block_on(outbox.acknowledge(&true, &lease, &receipt)),
        Err(MachineStoreError::Denied)
    );
    policy.allowed.store(true, Ordering::SeqCst);
    clock.0.store(110, Ordering::SeqCst);
    assert_eq!(
        block_on(outbox.acknowledge(&true, &lease, &receipt)),
        Err(MachineStoreError::Conflict)
    );
    assert!(
        block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
            .unwrap()
            .unwrap()
            .accepted
            .is_none()
    );
}

fn escalate(
    native: &SqliteProcessStore,
    scope: &ProcessScope,
    receipt: &MachineSagaReceipt,
    graph: bool,
) -> Vec<ProcessOutcome> {
    let start_bytes =
        block_on(native.read_payload(scope, receipt.commit().outcomes[0].payload_digest)).unwrap();
    let observation = ActionResultObservation {
        action_id: receipt.commit().actions[0].action_id.clone(),
        result: ActionResult::Unknown,
    };
    let input: InputId = id("inp_unknown");
    let facts = |kinds: Vec<ProcessOutcomeKind>, bytes: &[u8]| {
        kinds
            .into_iter()
            .enumerate()
            .map(|(i, kind)| {
                ProcessOutcomeFact::new(
                    id(&format!("out_unknown_{i}")),
                    CausationId::Input(input.clone()),
                    OutcomeActor::System,
                    LogicalTime(2),
                    kind,
                    ContentDigest::sha256(bytes),
                )
            })
            .collect::<Vec<_>>()
    };
    let (outcomes, bytes) = match definition(graph) {
        MachineSagaDefinition::Linear(d) => {
            let started: LinearSagaEvent = serde_json::from_slice(&start_bytes).unwrap();
            let before = replay(
                &d,
                &scope.tenant_id,
                &scope.process_id,
                std::slice::from_ref(&started),
            )
            .unwrap();
            let decision = apply_action_result(
                &d,
                &before.projection,
                scope.tenant_id.clone(),
                scope.process_id.clone(),
                &observation,
                None,
            )
            .unwrap();
            assert_eq!(decision.projection.status, SagaStatus::Escalated);
            let event = LinearSagaEvent::ActionResultObserved {
                input_id: input.clone(),
                observation,
                next_action_id: None,
            };
            assert_eq!(
                replay(
                    &d,
                    &scope.tenant_id,
                    &scope.process_id,
                    &[started, event.clone()]
                )
                .unwrap()
                .projection
                .status,
                SagaStatus::Escalated
            );
            let bytes = serde_json::to_vec(&event).unwrap();
            let kinds = event
                .observed_outcome_kinds()
                .iter()
                .chain(decision.planned_outcome_kinds())
                .copied()
                .collect();
            (
                decision
                    .build_required_outcomes(&event, scope, 3, &facts(kinds, &bytes))
                    .unwrap(),
                bytes,
            )
        }
        MachineSagaDefinition::Graph(d) => {
            let started: GraphSagaEvent = serde_json::from_slice(&start_bytes).unwrap();
            let before = replay_graph(
                &d,
                &scope.tenant_id,
                &scope.process_id,
                std::slice::from_ref(&started),
            )
            .unwrap();
            let decision = apply_graph_result(
                &d,
                &before,
                GraphSagaInput::ActionResult {
                    input_id: input.clone(),
                    observation,
                    next_action_id: None,
                },
            )
            .unwrap();
            assert_eq!(decision.projection.status, SagaStatus::Escalated);
            assert_eq!(
                replay_graph(
                    &d,
                    &scope.tenant_id,
                    &scope.process_id,
                    &[started, decision.event.clone()]
                )
                .unwrap()
                .status,
                SagaStatus::Escalated
            );
            let bytes = serde_json::to_vec(&decision.event).unwrap();
            let kinds = decision
                .event
                .observed_outcome_kinds()
                .iter()
                .chain(decision.planned_outcome_kinds())
                .copied()
                .collect();
            (
                decision
                    .build_required_outcomes(scope, 3, &facts(kinds, &bytes))
                    .unwrap(),
                bytes,
            )
        }
    };
    block_on(native.put_payload(scope, ContentDigest::sha256(&bytes), &bytes)).unwrap();
    let commit = AtomicProcessCommit::new(3, None, outcomes.clone(), vec![]).unwrap();
    block_on(native.commit(&commit)).unwrap();
    outcomes
}

#[cfg(unix)]
#[path = "support/machine_saga_crash.rs"]
mod crash;

#[async_trait]
impl ProcessCommitVerifier for NativeFaults {
    async fn verify_commit(
        &self,
        commit: &AtomicProcessCommit,
    ) -> Result<AtomicProcessCommitReceipt, PortError> {
        self.inner.verify_commit(commit).await
    }
}

#[path = "../../penelope-sqlite/tests/support/write_snapshot.rs"]
mod native_snapshot;
#[test]
fn native_verification_is_read_only_and_missing_commit_or_action_never_acknowledges_source() {
    for damaged in [None, Some("process_actions"), Some("process_commits")] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native.db");
        let native = process(&path);
        let receiver = make_receiver(Arc::clone(&native), Policy::new(true), false);
        let outbox = SqliteMachineOutbox::new(
            source(&directory.path().join("source.db"), 8),
            Arc::new(Clock(AtomicU64::new(100))),
            Arc::clone(&receiver),
            LIMITS,
        )
        .unwrap();
        let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
        let receipt = block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(1))).unwrap();
        let sql = rusqlite::Connection::open(&path).unwrap();
        if let Some(table) = damaged {
            sql.execute(&format!("DELETE FROM {table}"), []).unwrap();
        }
        let tables = [
            "process_heads",
            "process_usage",
            "process_outcomes",
            "process_actions",
            "process_commits",
            "process_payloads",
        ];
        let before = native_snapshot::snapshot(&sql, &tables);
        for table in tables {
            for operation in ["INSERT", "UPDATE", "DELETE"] {
                sql.execute_batch(&format!("CREATE TRIGGER deny_{table}_{operation} BEFORE {operation} ON {table} BEGIN SELECT RAISE(ABORT,'read only proof'); END;")).unwrap();
            }
        }
        let proof = block_on(native.verify_commit(receipt.commit()));
        let ack = block_on(outbox.acknowledge(&true, &lease, &receipt));
        if damaged.is_some() {
            assert!(proof.is_err());
            assert!(ack.is_err());
        } else {
            assert_eq!(proof.unwrap().committed_through_sequence, 2);
            ack.unwrap();
        }
        assert_eq!(native_snapshot::snapshot(&sql, &tables), before);
    }
}
