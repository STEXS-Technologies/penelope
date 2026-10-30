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
struct Stores {
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
        let binding =
            TaskEffectBinding::new(EffectDispatchRequest::new(action), definition(2)).unwrap();
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
        for offset in 1..45 {
            if supervisor.projection().status() == TaskStatus::Completed {
                break;
            }
            let next_tick = TaskTick::new(tick + offset);
            if supervisor.projection().status() == TaskStatus::Running {
                supervisor.poll(next_tick).unwrap();
            } else {
                supervisor.advance(next_tick).unwrap();
                if supervisor.projection().status() == TaskStatus::Running {
                    cut("worker-start");
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
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
        let outcome = VerifiedTaskEffectOutcome::verify(
            binding,
            &projection,
            projection.generation(),
            &bytes,
            &SumClassifier,
            None,
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
    for separate in modes() {
        let scope = scope();
        let definition = LinearSagaDefinition::new(
            scope.definition_id.clone(),
            scope.definition_version.clone(),
            scope.definition_digest,
            vec![step()],
        );
        let mut stores = Stores::new(separate, scope.clone());
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
        run(relay.commit(&stores.store, &result_commit)).unwrap();
        cut("result-commit");
        stores.reopen();
        assert!(
            run(relay.commit(&stores.store, &result_commit))
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
    for separate in modes() {
        let scope = scope();
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
        run(relay.commit(&stores.store, &result_commit)).unwrap();
        cut("result-commit");
        stores.reopen();
        assert!(
            run(relay.commit(&stores.store, &result_commit))
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
fn cut(point: &str) {
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
    use std::process::{Command, Stdio};
    for engine in ["linear", "graph"] {
        let test = format!(
            "sagas::full_{engine}_saga_replays_real_task_result_from_shared_and_separate_stores"
        );
        for separate in ["0", "1"] {
            for point in [
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
            ] {
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
                assert!(!child.wait().unwrap().success());
                let mut recovery = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", &test, "--nocapture"])
                    .env("PENELOPE_SAGA_CRASH_DIR", directory.path())
                    .env("PENELOPE_SAGA_SEPARATE", separate)
                    .env("PENELOPE_SAGA_RECOVER", "1")
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
