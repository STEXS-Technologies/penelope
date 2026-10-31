//! Full engine/store/runtime composition. Every replay event is read from the
//! durable payload store through an authoritative outcome reference.
use super::{Runtime, Supervisor, config, definition};
use penelope_core::task::{BlobRef, Digest, TaskStatus, TaskTick, runtime::TaskJournal};
use penelope_domain::*;
use penelope_executor::{
    composition::{TaskResultRelay, commit_task_effect},
    engine::*,
    graph::*,
};
use penelope_ports::{payloads::ProcessPayloadStore, tasks::*, *};
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore, SqliteTaskJournal};
use std::{
    future::Future,
    num::{NonZeroU16, NonZeroU32},
    path::PathBuf,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

fn id<T: std::str::FromStr>(text: &str) -> T {
    text.parse().ok().unwrap()
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn run<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(Instant::now() < deadline);
        std::thread::park_timeout(Duration::from_millis(2));
    }
}
struct Clock(u64);
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0))
    }
}
struct SumClassifier;
impl TaskOutputClassifier for SumClassifier {
    fn identity(&self) -> Digest {
        Digest([8; 32])
    }
    fn classify(
        &self,
        _binding: &TaskEffectBinding,
        output: BlobRef,
        bytes: &[u8],
    ) -> Result<ExternalEffectState, TaskEffectVerificationError> {
        if output.schema != 1 || bytes != 6_u64.to_le_bytes() {
            return Err(TaskEffectVerificationError::InvalidOutput);
        }
        Ok(ExternalEffectState::Succeeded)
    }
}
// Independent trusted fixture: this deliberately models an external authority,
// rather than treating the protected worker's completion as effect evidence.
struct SumEvidence {
    binding: TaskEffectBinding,
    generation: penelope_core::task::TaskGeneration,
    output: BlobRef,
}
impl TaskEffectEvidenceVerifier for SumEvidence {
    fn verify(
        &self,
        binding: &TaskEffectBinding,
        generation: penelope_core::task::TaskGeneration,
        output: BlobRef,
        bytes: &[u8],
    ) -> Result<ExternalEffectEvidence, TaskEffectVerificationError> {
        if binding != &self.binding
            || generation != self.generation
            || output != self.output
            || bytes != 6_u64.to_le_bytes()
        {
            return Err(TaskEffectVerificationError::BindingMismatch);
        }
        Ok(ExternalEffectEvidence {
            action_id: binding.request().action.action_id.clone(),
            effect_key: binding.request().effect_key.clone(),
            external_reference_id: None,
            state: ExternalEffectState::Succeeded,
        })
    }
}
struct Stores {
    effectful: bool,
    directory: PathBuf,
    _temporary: Option<tempfile::TempDir>,
    now: LogicalTime,
    process_path: PathBuf,
    task_path: PathBuf,
    store: SqliteProcessStore,
    scope: ProcessScope,
}
impl Stores {
    fn new(separate: bool, scope: ProcessScope) -> Self {
        let temporary = std::env::var_os("PENELOPE_SAGA_CRASH_DIR")
            .is_none()
            .then(|| tempfile::tempdir().unwrap());
        let directory = temporary.as_ref().map_or_else(
            || PathBuf::from(std::env::var_os("PENELOPE_SAGA_CRASH_DIR").unwrap()),
            |value| value.path().to_path_buf(),
        );
        let now = LogicalTime(if std::env::var_os("PENELOPE_SAGA_RECOVER").is_some() {
            2000
        } else {
            1
        });
        let task_path = directory.join("tasks.db");
        let process_path = if separate {
            directory.join("process.db")
        } else {
            task_path.clone()
        };
        let store = SqliteProcessStore::open(
            &process_path,
            SqliteLimits::default(),
            Arc::new(Clock(now.0)),
            100,
            8,
        )
        .unwrap();
        Self {
            effectful: false,
            directory,
            _temporary: temporary,
            now,
            process_path,
            task_path,
            store,
            scope,
        }
    }
    fn reopen(&mut self) {
        self.store = SqliteProcessStore::open(
            &self.process_path,
            SqliteLimits::default(),
            Arc::new(Clock(self.now.0)),
            100,
            8,
        )
        .unwrap();
    }
    fn persist(&self, event: &impl serde::Serialize) -> ContentDigest {
        let bytes = serde_json::to_vec(event).unwrap();
        let digest = ContentDigest::sha256(&bytes);
        assert_eq!(
            run(self.store.put_payload(&self.scope, digest, &bytes)).unwrap(),
            digest
        );
        digest
    }
    fn facts(
        &self,
        input_id: &InputId,
        kinds: impl Iterator<Item = ProcessOutcomeKind>,
        digest: ContentDigest,
        sequence: u64,
    ) -> Vec<ProcessOutcomeFact> {
        kinds
            .enumerate()
            .map(|(index, kind)| {
                ProcessOutcomeFact::new(
                    id(&format!(
                        "out_saga_{}",
                        sequence + u64::try_from(index).unwrap()
                    )),
                    CausationId::Input(input_id.clone()),
                    OutcomeActor::System,
                    LogicalTime(sequence),
                    kind,
                    digest,
                )
            })
            .collect()
    }
    fn execute(&mut self, commit: &AtomicProcessCommit) -> TaskResultRelay {
        let action = commit.actions.first().unwrap().clone();
        let mut task_definition = definition(2);
        if self.effectful {
            task_definition.effects = penelope_core::task::EffectContract::ReconcileRequired;
        }
        let binding =
            TaskEffectBinding::new(EffectDispatchRequest::new(action), task_definition).unwrap();
        // Lose the initial process acknowledgement, reopen, then reconstruct the
        // exact commit receipt before any worker can be admitted.
        run(self.store.commit(commit)).unwrap();
        cut("process-commit");
        self.reopen();
        let committed = run(commit_task_effect(&self.store, commit, binding.clone())).unwrap();
        let claim = OutboxClaimRequest::new(
            self.scope.clone(),
            id("pri_saga"),
            NonZeroU16::new(1).unwrap(),
        )
        .unwrap();
        let lease = run(self.store.claim(&claim)).unwrap().pop();
        cut("outbox-claim");
        let journal = SqliteTaskJournal::open(
            &self.task_path,
            binding.definition(),
            SqliteLimits::default(),
        )
        .unwrap();
        let authority = SqliteTaskJournal::open(
            &self.task_path,
            binding.definition(),
            SqliteLimits::default(),
        )
        .unwrap();
        let runtime = Runtime::new(authority, config(&self.directory)).unwrap();
        let tick = if self.now.0 == 1 { 10 } else { 100 };
        let mut supervisor: Supervisor = if let Some(lease) = lease {
            let mut accepted = Supervisor::restore(
                journal,
                runtime,
                binding.scope(),
                binding.definition(),
                TaskTick::new(tick),
            )
            .unwrap();
            if accepted.projection().status() == TaskStatus::Empty {
                accepted
                    .submit(&[0, 1, 2, 3], 1, TaskTick::new(tick))
                    .unwrap();
            }
            cut("task-intent");
            let (accepted_journal, accepted_runtime) = accepted.into_parts();
            run(committed.accept(
                &self.store,
                &claim,
                &lease,
                self.now,
                accepted_journal,
                accepted_runtime,
                &[0, 1, 2, 3],
                1,
                TaskTick::new(tick),
            ))
            .unwrap()
        } else {
            committed
                .recover(journal, runtime, &[0, 1, 2, 3], 1, TaskTick::new(tick))
                .unwrap()
        };
        cut("outbox-ack");
        if self.effectful {
            let expected = match std::env::var("PENELOPE_SAGA_EXPECT_REPLAY").ok().as_deref() {
                Some("reconciliation-required" | "reconciliation-unknown") => {
                    Some(TaskStatus::Reconciling)
                }
                Some("restart-authorized") => Some(TaskStatus::Reconciled),
                _ => None,
            };
            if let Some(expected) = expected {
                assert_eq!(supervisor.projection().status(), expected);
                assert_eq!(
                    supervisor.projection().generation(),
                    penelope_core::task::TaskGeneration::new(1)
                );
                assert_eq!(supervisor.projection().output(), None);
            }
        }
        let first_offset = if self.effectful {
            use penelope_core::task::Reconciliation;
            if supervisor.projection().status() == TaskStatus::Pending {
                supervisor.advance(TaskTick::new(tick + 1)).unwrap();
                assert_eq!(supervisor.projection().status(), TaskStatus::Running);
                cut("initial-worker-start");
                let (interrupted_journal, interrupted_runtime) = supervisor.into_parts();
                supervisor = Supervisor::restore(
                    interrupted_journal,
                    interrupted_runtime,
                    binding.scope(),
                    binding.definition(),
                    TaskTick::new(tick + 2),
                )
                .unwrap();
            }
            if supervisor.projection().status() == TaskStatus::Faulted {
                supervisor.advance(TaskTick::new(tick + 3)).unwrap();
            }
            if supervisor.projection().status() == TaskStatus::Reconciling {
                cut("reconciliation-required");
                let generation = supervisor.projection().generation();
                supervisor
                    .reconcile(
                        binding.scope(),
                        generation,
                        Reconciliation::Unknown {
                            evidence: Digest([3; 32]),
                        },
                        None,
                        TaskTick::new(tick + 3),
                    )
                    .unwrap();
                cut("reconciliation-unknown");
                for offset in 4..7 {
                    assert!(!supervisor.advance(TaskTick::new(tick + offset)).unwrap());
                    assert_eq!(supervisor.projection().generation(), generation);
                    assert_eq!(supervisor.projection().status(), TaskStatus::Reconciling);
                }
                let (blocked_journal, blocked_runtime) = supervisor.into_parts();
                supervisor = Supervisor::restore(
                    blocked_journal,
                    blocked_runtime,
                    binding.scope(),
                    binding.definition(),
                    TaskTick::new(tick + 7),
                )
                .unwrap();
                assert_eq!(supervisor.projection().status(), TaskStatus::Reconciling);
                assert_eq!(supervisor.projection().generation(), generation);
                // Trusted fixture policy authorizes retry independently of worker output.
                // These evidence digests identify the fixture; they are not cryptographic proof.
                supervisor
                    .reconcile(
                        binding.scope(),
                        generation,
                        Reconciliation::RestartAuthorized {
                            evidence: Digest([4; 32]),
                        },
                        None,
                        TaskTick::new(tick + 7),
                    )
                    .unwrap();
                cut("restart-authorized");
            }
            8
        } else {
            1
        };
        complete_task(&mut supervisor, tick, first_offset);
        cut("task-complete");
        drop(supervisor);
        self.reopen();
        let relay = self.read_result(&binding);
        cut("result-verified");
        relay
    }
    fn read_result(&self, binding: &TaskEffectBinding) -> TaskResultRelay {
        let journal = SqliteTaskJournal::open(
            &self.task_path,
            binding.definition(),
            SqliteLimits::default(),
        )
        .unwrap();
        let authority = SqliteTaskJournal::open(
            &self.task_path,
            binding.definition(),
            SqliteLimits::default(),
        )
        .unwrap();
        let runtime = Runtime::new(authority, config(&self.directory)).unwrap();
        let recovered = Supervisor::restore(
            journal,
            runtime,
            binding.scope(),
            binding.definition(),
            TaskTick::new(150),
        )
        .unwrap();
        let projection = *recovered.projection();
        let (mut recovered_journal, _runtime) = recovered.into_parts();
        let mut bytes = [0; 8];
        recovered_journal
            .read_blob(binding.scope(), projection.output().unwrap(), &mut bytes)
            .unwrap();
        let evidence = SumEvidence {
            binding: binding.clone(),
            generation: projection.generation(),
            output: BlobRef {
                digest: Digest::of(&6_u64.to_le_bytes()),
                bytes: 8,
                schema: 1,
            },
        };
        let verifier = self
            .effectful
            .then_some(&evidence as &dyn TaskEffectEvidenceVerifier);
        if self.effectful {
            assert_eq!(
                VerifiedTaskEffectOutcome::verify(
                    binding,
                    &projection,
                    projection.generation(),
                    &bytes,
                    &SumClassifier,
                    None
                ),
                Err(TaskEffectVerificationError::MissingEvidence)
            );
            assert!(projection.generation() > penelope_core::task::TaskGeneration::new(1));
        }
        let outcome = VerifiedTaskEffectOutcome::verify(
            binding,
            &projection,
            projection.generation(),
            &bytes,
            &SumClassifier,
            verifier,
        )
        .unwrap();
        TaskResultRelay::new(outcome).unwrap()
    }
    fn replay_events<T: serde::de::DeserializeOwned>(&self) -> Vec<T> {
        let request =
            OutcomeReplayRequest::new(self.scope.clone(), 0, NonZeroU16::new(32).unwrap()).unwrap();
        let page = run(self.store.read_outcomes(&request)).unwrap();
        assert!(page.next_sequence.is_none());
        page.outcomes
            .iter()
            .filter(|outcome| outcome.kind == ProcessOutcomeKind::InputAccepted)
            .map(|outcome| {
                let bytes =
                    run(self.store.read_payload(&self.scope, outcome.payload_digest)).unwrap();
                serde_json::from_slice(&bytes).unwrap()
            })
            .collect()
    }
}
fn step() -> StepPlan {
    StepPlan {
        step_id: id("stp_sum"),
        action_kind: ProcessActionKind::ExternalEffect,
        payload_digest: ContentDigest::sha256(&[0, 1, 2, 3]),
        retry_policy: RetryPolicy::no_retry(),
        compensation: None,
    }
}
fn scope() -> ProcessScope {
    ProcessScope::new(
        id("tnt_saga"),
        id("prc_saga"),
        id("def_saga"),
        id("dfv_one"),
        ContentDigest([9; 32]),
    )
}
fn start_input(scope: &ProcessScope) -> ProcessInput {
    ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id("inp_start"),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([1; 32]),
    )
}

#[test]
fn full_linear_saga_replays_real_task_result_from_shared_and_separate_stores() {
    run_linear_saga(false);
}
#[test]
fn reconciled_linear_saga_blocks_unknown_then_requires_independent_result_evidence() {
    run_linear_saga(true);
}
fn run_linear_saga(effectful: bool) {
    run_linear_saga_with_source(effectful, &scope(), |_, _, commit| commit);
}
// Reuse the complete engine/runtime/recovery contract for independently verified
// canonical-source integrations, without duplicating this workflow in a client.
pub(crate) fn run_linear_saga_with_source(
    effectful: bool,
    process_scope: &ProcessScope,
    attach_source: impl Fn(
        &SqliteProcessStore,
        &ProcessScope,
        AtomicProcessCommit,
    ) -> AtomicProcessCommit,
) {
    for separate in modes() {
        let scope = process_scope.clone();
        let definition = LinearSagaDefinition::new(
            scope.definition_id.clone(),
            scope.definition_version.clone(),
            scope.definition_digest,
            vec![step()],
        );
        let mut stores = Stores::new(separate, scope.clone());
        stores.effectful = effectful;
        let input = LinearSagaInput::Start {
            input_id: id("inp_start"),
            action_id: id("act_sum"),
        };
        let event = input.to_event(&definition);
        let started = decide(
            &definition,
            None,
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            &input,
        )
        .unwrap();
        let digest = stores.persist(&event);
        cut("start-payload");
        let facts = stores.facts(
            &id("inp_start"),
            event
                .observed_outcome_kinds()
                .iter()
                .chain(started.planned_outcome_kinds())
                .copied(),
            digest,
            0,
        );
        let outcomes = started
            .build_required_outcomes(&event, &scope, 0, &facts)
            .unwrap();
        let sequence = u64::try_from(outcomes.len()).unwrap();
        let commit = AtomicProcessCommit::new(
            0,
            Some(start_input(&scope)),
            outcomes,
            vec![started.next_action.clone().unwrap()],
        )
        .unwrap();
        let commit = attach_source(&stores.store, &scope, commit);
        let relay = stores.execute(&commit);
        // Reconstruct the running saga from durable events before applying result.
        let running = replay(
            &definition,
            &scope.tenant_id,
            &scope.process_id,
            &stores.replay_events(),
        )
        .unwrap();
        if running.projection.status == SagaStatus::Completed {
            assert!(running.next_action.is_none());
            assert_eq!(stores.replay_events::<LinearSagaEvent>().len(), 2);
            continue;
        }
        let result_input = relay.linear_input(None);
        let result_event = result_input.to_event(&definition);
        let finished = decide(
            &definition,
            Some(&running.projection),
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            &result_input,
        )
        .unwrap();
        assert_eq!(finished.projection.status, SagaStatus::Completed);
        let result_digest = stores.persist(&result_event);
        cut("result-payload");
        let result_facts = stores.facts(
            &relay.input().input_id,
            result_event
                .observed_outcome_kinds()
                .iter()
                .chain(finished.planned_outcome_kinds())
                .copied(),
            result_digest,
            sequence,
        );
        let result_outcomes = finished
            .build_required_outcomes(&result_event, &scope, sequence, &result_facts)
            .unwrap();
        let result_commit = AtomicProcessCommit::new(
            sequence,
            Some(relay.input().clone()),
            result_outcomes,
            vec![],
        )
        .unwrap();
        let denied = RelayPolicy {
            scope: scope.clone(),
            allow: false,
        };
        assert!(
            run(penelope_executor::services::relay_task_result(
                &denied,
                &id("pri_relay"),
                &relay,
                &stores.store,
                &result_commit
            ))
            .is_err()
        );
        let pending_result =
            OutcomeReplayRequest::new(scope.clone(), sequence, NonZeroU16::new(1).unwrap())
                .unwrap();
        assert!(
            run(stores.store.read_outcomes(&pending_result))
                .unwrap()
                .outcomes
                .is_empty()
        );
        let allowed = RelayPolicy {
            scope: scope.clone(),
            allow: true,
        };
        run(penelope_executor::services::relay_task_result(
            &allowed,
            &id("pri_relay"),
            &relay,
            &stores.store,
            &result_commit,
        ))
        .unwrap();
        cut("result-commit");
        stores.reopen();
        assert!(
            run(penelope_executor::services::relay_task_result(
                &allowed,
                &id("pri_relay"),
                &relay,
                &stores.store,
                &result_commit
            ))
            .unwrap()
            .duplicate_input
        );
        let reconstructed = replay(
            &definition,
            &scope.tenant_id,
            &scope.process_id,
            &stores.replay_events(),
        )
        .unwrap();
        assert_eq!(reconstructed.projection, finished.projection);
        assert!(reconstructed.next_action.is_none());
        assert_eq!(stores.replay_events::<LinearSagaEvent>().len(), 2);
    }
}

#[test]
fn full_graph_saga_replays_real_task_result_from_shared_and_separate_stores() {
    run_graph_saga(false);
}
#[test]
fn reconciled_graph_saga_blocks_unknown_then_requires_independent_result_evidence() {
    run_graph_saga(true);
}
fn run_graph_saga(effectful: bool) {
    run_graph_saga_with_source(effectful, &scope(), |_, _, commit| commit);
}
// Reuse the complete engine/runtime/recovery contract for independently verified
// canonical-source integrations, without duplicating this workflow in a client.
pub(crate) fn run_graph_saga_with_source(
    effectful: bool,
    process_scope: &ProcessScope,
    attach_source: impl Fn(
        &SqliteProcessStore,
        &ProcessScope,
        AtomicProcessCommit,
    ) -> AtomicProcessCommit,
) {
    for separate in modes() {
        let scope = process_scope.clone();
        let definition = ProcessGraphDefinition {
            definition_id: scope.definition_id.clone(),
            definition_version: scope.definition_version.clone(),
            definition_digest: scope.definition_digest,
            steps: vec![step()],
            entry_step_id: id("stp_sum"),
            transitions: vec![GraphTransition {
                from_step: id("stp_sum"),
                on: GraphTransitionOutcome::Succeeded,
                to_step: None,
            }],
            max_step_visits: NonZeroU32::new(1).unwrap(),
        };
        let mut stores = Stores::new(separate, scope.clone());
        stores.effectful = effectful;
        let started = start_graph(
            &definition,
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            GraphSagaInput::Start {
                input_id: id("inp_start"),
                action_id: id("act_sum"),
            },
        )
        .unwrap();
        let digest = stores.persist(&started.event);
        cut("start-payload");
        let facts = stores.facts(
            started.event.input_id(),
            started
                .event
                .observed_outcome_kinds()
                .iter()
                .chain(started.planned_outcome_kinds())
                .copied(),
            digest,
            0,
        );
        let outcomes = started.build_required_outcomes(&scope, 0, &facts).unwrap();
        let sequence = u64::try_from(outcomes.len()).unwrap();
        let commit = AtomicProcessCommit::new(
            0,
            Some(start_input(&scope)),
            outcomes,
            vec![started.next_action.clone().unwrap()],
        )
        .unwrap();
        let commit = attach_source(&stores.store, &scope, commit);
        let relay = stores.execute(&commit);
        let running = replay_graph(
            &definition,
            &scope.tenant_id,
            &scope.process_id,
            &stores.replay_events(),
        )
        .unwrap();
        if running.status == SagaStatus::Completed {
            assert!(running.active_action_id.is_none());
            assert_eq!(stores.replay_events::<GraphSagaEvent>().len(), 2);
            continue;
        }
        let finished = apply_graph_result(&definition, &running, relay.graph_input(None)).unwrap();
        assert_eq!(finished.projection.status, SagaStatus::Completed);
        let result_digest = stores.persist(&finished.event);
        cut("result-payload");
        let result_facts = stores.facts(
            finished.event.input_id(),
            finished
                .event
                .observed_outcome_kinds()
                .iter()
                .chain(finished.planned_outcome_kinds())
                .copied(),
            result_digest,
            sequence,
        );
        let result_outcomes = finished
            .build_required_outcomes(&scope, sequence, &result_facts)
            .unwrap();
        let result_commit = AtomicProcessCommit::new(
            sequence,
            Some(relay.input().clone()),
            result_outcomes,
            vec![],
        )
        .unwrap();
        let denied = RelayPolicy {
            scope: scope.clone(),
            allow: false,
        };
        assert!(
            run(penelope_executor::services::relay_task_result(
                &denied,
                &id("pri_relay"),
                &relay,
                &stores.store,
                &result_commit
            ))
            .is_err()
        );
        let pending_result =
            OutcomeReplayRequest::new(scope.clone(), sequence, NonZeroU16::new(1).unwrap())
                .unwrap();
        assert!(
            run(stores.store.read_outcomes(&pending_result))
                .unwrap()
                .outcomes
                .is_empty()
        );
        let allowed = RelayPolicy {
            scope: scope.clone(),
            allow: true,
        };
        run(penelope_executor::services::relay_task_result(
            &allowed,
            &id("pri_relay"),
            &relay,
            &stores.store,
            &result_commit,
        ))
        .unwrap();
        cut("result-commit");
        stores.reopen();
        assert!(
            run(penelope_executor::services::relay_task_result(
                &allowed,
                &id("pri_relay"),
                &relay,
                &stores.store,
                &result_commit
            ))
            .unwrap()
            .duplicate_input
        );
        let reconstructed = replay_graph(
            &definition,
            &scope.tenant_id,
            &scope.process_id,
            &stores.replay_events(),
        )
        .unwrap();
        assert_eq!(reconstructed, finished.projection);
        assert!(reconstructed.active_action_id.is_none());
        assert_eq!(stores.replay_events::<GraphSagaEvent>().len(), 2);
    }
}

fn modes() -> Vec<bool> {
    std::env::var("PENELOPE_SAGA_SEPARATE")
        .map_or_else(|_missing| vec![false, true], |value| vec![value == "1"])
}
pub(crate) fn cut(point: &str) {
    if std::env::var("PENELOPE_SAGA_CUT").ok().as_deref() == Some(point) {
        let directory = PathBuf::from(std::env::var_os("PENELOPE_SAGA_CRASH_DIR").unwrap());
        std::fs::write(directory.join("crash-ready"), point).unwrap();
        loop {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn killed_coordinators_recover_linear_and_graph_sagas_at_durable_boundaries() {
    crash_matrix(false);
}
#[test]
fn killed_coordinators_recover_effectful_sagas_at_reconciliation_boundaries() {
    crash_matrix(true);
}
fn crash_matrix(effectful: bool) {
    crash_matrix_with_source(
        effectful,
        |engine| {
            if effectful {
                format!(
                    "sagas::reconciled_{engine}_saga_blocks_unknown_then_requires_independent_result_evidence"
                )
            } else {
                format!(
                    "sagas::full_{engine}_saga_replays_real_task_result_from_shared_and_separate_stores"
                )
            }
        },
        &[],
    );
}
pub(crate) fn crash_matrix_with_source(
    effectful: bool,
    test_for_engine: impl Fn(&str) -> String,
    source_points: &[&str],
) {
    use std::{
        os::unix::process::ExitStatusExt,
        process::{Command, Stdio},
    };
    for engine in ["linear", "graph"] {
        let test = test_for_engine(engine);
        let mut points = vec![
            "start-payload",
            "process-commit",
            "outbox-claim",
            "task-intent",
            "outbox-ack",
            "worker-start",
            "task-complete",
            "result-verified",
            "result-payload",
            "result-commit",
        ];
        if effectful {
            points.extend([
                "initial-worker-start",
                "reconciliation-required",
                "reconciliation-unknown",
                "restart-authorized",
            ]);
        }
        points.extend_from_slice(source_points);
        for separate in ["0", "1"] {
            for point in &points {
                let directory = tempfile::tempdir().unwrap();
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", &test, "--nocapture"])
                    .env("PENELOPE_SAGA_CRASH_DIR", directory.path())
                    .env("PENELOPE_SAGA_SEPARATE", separate)
                    .env("PENELOPE_SAGA_CUT", point)
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while !directory.path().join("crash-ready").exists() {
                    if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
                        let _kill_result = child.kill();
                        let _wait_result = child.wait();
                        panic!(
                            "{engine}/{separate}/{point}: coordinator never reached crash boundary"
                        );
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                child.kill().unwrap();
                assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
                let mut recovery = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", &test, "--nocapture"])
                    .env("PENELOPE_SAGA_CRASH_DIR", directory.path())
                    .env("PENELOPE_SAGA_SEPARATE", separate)
                    .env("PENELOPE_SAGA_RECOVER", "1")
                    .env("PENELOPE_SAGA_EXPECT_REPLAY", point)
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap();
                let recovery_deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    if let Some(status) = recovery.try_wait().unwrap() {
                        assert!(
                            status.success(),
                            "{engine}/{separate}/{point}: reconstruction failed"
                        );
                        break;
                    }
                    if Instant::now() >= recovery_deadline {
                        let _kill_result = recovery.kill();
                        let _wait_result = recovery.wait();
                        panic!("{engine}/{separate}/{point}: reconstruction timed out");
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
}

#[path = "services.rs"]
mod services;

struct RelayPolicy {
    scope: ProcessScope,
    allow: bool,
}
#[async_trait::async_trait]
impl ProcessAuthorizer for RelayPolicy {
    async fn authorize(
        &self,
        request: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        assert_eq!(request.scope, self.scope);
        assert_eq!(request.principal_id, id("pri_relay"));
        assert_eq!(
            request.operation,
            ProcessAuthorizationOperation::RelayTaskResult
        );
        Ok(if self.allow {
            ProcessAuthorizationDecision::Authorized
        } else {
            ProcessAuthorizationDecision::Denied
        })
    }
}

#[path = "cancellation.rs"]
mod cancellation;

fn complete_task(supervisor: &mut Supervisor, tick: u64, first_offset: u64) {
    // Host scheduling must not consume the fixture's logical execution budget.
    // Preserve its original logical transitions, then keep polling at the last
    // tick until completion or a bounded wall-clock deadline.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut offset = first_offset;
    while supervisor.projection().status() != TaskStatus::Completed {
        assert!(
            Instant::now() < deadline,
            "saga worker did not complete: {:?}",
            supervisor.projection().status()
        );
        let next_tick = TaskTick::new(tick + offset);
        if supervisor.projection().status() == TaskStatus::Running {
            supervisor.poll(next_tick).unwrap();
        } else {
            supervisor.advance(next_tick).unwrap();
            if supervisor.projection().status() == TaskStatus::Running {
                cut("worker-start");
            }
        }
        offset = offset.saturating_add(1).min(44);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
}

#[test]
fn saga_completion_waits_for_a_delayed_real_worker() {
    let directory = tempfile::tempdir().unwrap();
    let definition = definition(2);
    let input = [28, 1, 2, 3];
    let scope = penelope_core::task::TaskScope {
        tenant: penelope_core::task::TaskTenantId::from_digest(Digest([1; 32])),
        task: penelope_core::task::TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: penelope_core::task::TaskInputDigest::from_digest(Digest::of(&input)),
    };
    let path = directory.path().join("delayed.db");
    let journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let runtime = Runtime::new(authority, config(directory.path())).unwrap();
    let mut supervisor: Supervisor =
        Supervisor::restore(journal, runtime, scope, definition, TaskTick::ZERO).unwrap();
    supervisor.submit(&input, 1, TaskTick::ZERO).unwrap();
    let started = Instant::now();
    supervisor.advance(TaskTick::new(1)).unwrap();
    complete_task(&mut supervisor, 0, 2);
    println!(
        "delayed-worker elapsed_ms={} status={:?}",
        started.elapsed().as_millis(),
        supervisor.projection().status()
    );
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(supervisor.projection().last_tick() <= TaskTick::new(44));
    assert_eq!(
        supervisor.projection().generation(),
        penelope_core::task::TaskGeneration::new(1)
    );
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
    let output = supervisor.projection().output().unwrap();
    let (mut completed_journal, _runtime) = supervisor.into_parts();
    let mut bytes = [0; 8];
    completed_journal
        .read_blob(scope, output, &mut bytes)
        .unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 6);
}
