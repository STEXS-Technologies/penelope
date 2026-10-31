//! Portable composition demonstration with durable stores and a simulated runtime.
//! Use the process-runtime example for protected execution.
#[path = "../../tests/support/tasks.rs"]
mod support;
use async_trait::async_trait;
use penelope_core::task::{
    self,
    runtime::{TaskClock, TaskJournal},
    *,
};
use penelope_domain::*;
use penelope_executor::{
    composition::{TaskResultRelay, commit_task_effect},
    engine::*,
    services::{claim_outbox, drive_task_once, relay_task_result},
    task::TaskSupervisor,
};
use penelope_ports::{payloads::ProcessPayloadStore, tasks::*, *};
use penelope_sqlite::{SqliteLeaseClock, SqliteLimits, SqliteProcessStore, SqliteTaskJournal};
use std::{
    error::Error,
    future::Future,
    num::NonZeroU16,
    path::Path,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn invalid(message: &str) -> std::io::Error {
    std::io::Error::other(message)
}
fn id<T: std::str::FromStr>(text: &str) -> Result<T>
where
    T::Err: Error + 'static,
{
    Ok(text.parse()?)
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> std::result::Result<LogicalTime, PortError> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_clock| PortError::Unavailable)?;
        Ok(LogicalTime(
            u64::try_from(elapsed.as_millis()).map_err(|_overflow| PortError::Invariant)?,
        ))
    }
}
struct Ticks {
    base: u64,
    origin: Instant,
}
impl TaskClock for Ticks {
    fn now(&self) -> TaskTick {
        TaskTick::new(
            self.base.saturating_add(
                u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX),
            ),
        )
    }
}
struct LocalPolicy {
    scope: ProcessScope,
    principal: PrincipalId,
}
#[async_trait]
impl ProcessAuthorizer for LocalPolicy {
    async fn authorize(
        &self,
        request: &ProcessAuthorizationRequest,
    ) -> std::result::Result<ProcessAuthorizationDecision, PortError> {
        Ok(
            if request.scope == self.scope
                && request.principal_id == self.principal
                && matches!(
                    request.operation,
                    ProcessAuthorizationOperation::Start
                        | ProcessAuthorizationOperation::Dispatch
                        | ProcessAuthorizationOperation::DriveTask
                        | ProcessAuthorizationOperation::RelayTaskResult
                )
            {
                ProcessAuthorizationDecision::Authorized
            } else {
                ProcessAuthorizationDecision::Denied
            },
        )
    }
}
struct Sum;
impl TaskOutputClassifier for Sum {
    fn identity(&self) -> task::Digest {
        task::Digest::of(b"example.sum.u64-le.v1")
    }
    fn classify(
        &self,
        _binding: &TaskEffectBinding,
        output: BlobRef,
        bytes: &[u8],
    ) -> std::result::Result<ExternalEffectState, TaskEffectVerificationError> {
        if output.schema != 1 || bytes != 6_u64.to_le_bytes() {
            return Err(TaskEffectVerificationError::InvalidOutput);
        }
        Ok(ExternalEffectState::Succeeded)
    }
}
fn persist(
    store: &SqliteProcessStore,
    scope: &ProcessScope,
    event: &LinearSagaEvent,
) -> Result<ContentDigest> {
    let bytes = serde_json::to_vec(event)?;
    let digest = ContentDigest::sha256(&bytes);
    if wait(store.put_payload(scope, digest, &bytes))? != digest {
        return Err(invalid("payload receipt differs").into());
    }
    Ok(digest)
}
fn batch(
    decision: &SagaDecision,
    event: &LinearSagaEvent,
    scope: &ProcessScope,
    input: ProcessInput,
    sequence: u64,
    digest: ContentDigest,
) -> Result<AtomicProcessCommit> {
    let input_id = event
        .input_id()
        .ok_or_else(|| invalid("event has no input"))?;
    let mut facts = Vec::new();
    for (offset, kind) in event
        .observed_outcome_kinds()
        .iter()
        .chain(decision.planned_outcome_kinds())
        .enumerate()
    {
        let number = sequence
            .checked_add(u64::try_from(offset)?)
            .ok_or_else(|| invalid("sequence overflow"))?;
        facts.push(ProcessOutcomeFact::new(
            id(&format!("out_example_{number}"))?,
            CausationId::Input(input_id.clone()),
            OutcomeActor::System,
            LogicalTime(0),
            *kind,
            digest,
        ));
    }
    let outcomes = decision.build_required_outcomes(event, scope, sequence, &facts)?;
    Ok(AtomicProcessCommit::new(
        sequence,
        Some(input),
        outcomes,
        decision.next_action.iter().cloned().collect(),
    )?)
}
fn events(store: &SqliteProcessStore, scope: &ProcessScope) -> Result<(Vec<LinearSagaEvent>, u64)> {
    let request = OutcomeReplayRequest::new(
        scope.clone(),
        0,
        NonZeroU16::new(32).ok_or_else(|| invalid("invalid limit"))?,
    )?;
    let page = wait(store.read_outcomes(&request))?;
    if page.next_sequence.is_some() {
        return Err(invalid("example replay limit exceeded").into());
    }
    let next = u64::try_from(page.outcomes.len())?;
    let mut events = Vec::new();
    for outcome in page
        .outcomes
        .iter()
        .filter(|outcome| outcome.kind == ProcessOutcomeKind::InputAccepted)
    {
        let bytes = wait(store.read_payload(scope, outcome.payload_digest))?;
        events.push(serde_json::from_slice(&bytes)?);
    }
    Ok((events, next))
}
pub fn run() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let root = arguments
        .next()
        .ok_or_else(|| invalid("usage: saga_recoverable_task STATE_DIRECTORY"))?;
    if arguments.next().is_some() {
        return Err(invalid("unexpected argument").into());
    }
    let root = Path::new(&root);
    std::fs::create_dir_all(root)?;
    // This CLI's fixed demo principal illustrates scope authorization. A hosted
    // application must inject its authenticated principal and policy instead.
    let principal = id("pri_demo")?;
    let scope = ProcessScope::new(
        id("tnt_example")?,
        id("prc_sum")?,
        id("def_sum")?,
        id("dfv_one")?,
        ContentDigest([1; 32]),
    );
    let policy = LocalPolicy {
        scope: scope.clone(),
        principal,
    };
    let store = SqliteProcessStore::open(
        &root.join("process.db"),
        SqliteLimits::default(),
        Arc::new(Clock),
        30_000,
        8,
    )?;
    let input = [0, 1, 2, 3];
    let saga = LinearSagaDefinition::new(
        scope.definition_id.clone(),
        scope.definition_version.clone(),
        scope.definition_digest,
        vec![StepPlan {
            step_id: id("stp_sum")?,
            action_kind: ProcessActionKind::ExternalEffect,
            payload_digest: ContentDigest::sha256(&input),
            retry_policy: RetryPolicy::no_retry(),
            compensation: None,
        }],
    );
    wait(policy.authorize(&ProcessAuthorizationRequest {
        scope: scope.clone(),
        principal_id: policy.principal.clone(),
        operation: ProcessAuthorizationOperation::Start,
    }))?
    .require_authorized()?;
    let start_input = LinearSagaInput::Start {
        input_id: id("inp_start")?,
        action_id: id("act_sum")?,
    };
    let start_event = start_input.to_event(&saga);
    let started = decide(
        &saga,
        None,
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        &start_input,
    )?;
    let digest = persist(&store, &scope, &start_event)?;
    let start = batch(
        &started,
        &start_event,
        &scope,
        ProcessInput::new(
            scope.tenant_id.clone(),
            scope.process_id.clone(),
            id("inp_start")?,
            ProcessInputKind::CanonicalEvent,
            ContentDigest::sha256(b"example.start.v1"),
        ),
        0,
        digest,
    )?;
    let action = started
        .next_action
        .ok_or_else(|| invalid("start planned no action"))?;
    let task_definition =
        support::definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let binding = TaskEffectBinding::new(EffectDispatchRequest::new(action), task_definition)?;
    let committed = wait(commit_task_effect(&store, &start, binding.clone()))?;
    let (recorded, sequence) = events(&store, &scope)?;
    let running =
        penelope_executor::engine::replay(&saga, &scope.tenant_id, &scope.process_id, &recorded)?;
    if running.projection.status == SagaStatus::Completed {
        println!("Saga already completed; no task dispatched");
        return Ok(());
    }
    let mut journal = SqliteTaskJournal::open(
        &root.join("tasks.db"),
        task_definition,
        SqliteLimits::default(),
    )?;
    let mut frontier = 0;
    for history_sequence in 0..task_definition.budget.max_history_records {
        let Some(record) = journal.read(binding.scope(), TaskSequence::new(history_sequence))?
        else {
            break;
        };
        frontier = record.tick.get();
    }
    let ticks = Ticks {
        base: frontier
            .checked_add(1)
            .ok_or_else(|| invalid("task clock overflow"))?,
        origin: Instant::now(),
    };
    let runtime = support::SumRuntime::default();
    let claim = OutboxClaimRequest::new(
        scope.clone(),
        policy.principal.clone(),
        NonZeroU16::new(1).ok_or_else(|| invalid("invalid claim limit"))?,
    )?;
    let lease = wait(claim_outbox(
        &policy,
        &store,
        &claim,
        SqliteLeaseClock::now(&Clock)?,
    ))?
    .pop();
    let mut supervisor: TaskSupervisor<_, _, 64> = if let Some(lease) = lease {
        wait(committed.accept(
            &store,
            &claim,
            &lease,
            SqliteLeaseClock::now(&Clock)?,
            journal,
            runtime,
            &input,
            1,
            ticks.now(),
        ))?
    } else {
        committed.recover(journal, runtime, &input, 1, ticks.now())?
    };
    let mut reconstructed = false;
    for _ in 0..5000 {
        let report = wait(drive_task_once(
            &policy,
            &policy.principal,
            &committed,
            &mut supervisor,
            &ticks,
        ))?;
        if report.status == TaskStatus::Completed {
            break;
        }
        if !reconstructed && supervisor.projection().checkpoint().is_some() {
            // Demonstrate a coordinator interruption after a durable checkpoint.
            // The simulated runtime fences its old generation on recovery.
            let (saved_journal, saved_runtime) = supervisor.into_parts();
            supervisor = committed.recover(saved_journal, saved_runtime, &input, 1, ticks.now())?;
            reconstructed = true;
        }
        if matches!(
            report.status,
            TaskStatus::Exhausted | TaskStatus::Reconciling
        ) {
            return Err(invalid("task blocked; preserve stores and inspect evidence").into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let projection = *supervisor.projection();
    let output = projection
        .output()
        .ok_or_else(|| invalid("bounded worker loop did not complete"))?;
    let (mut completed_journal, completed_runtime) = supervisor.into_parts();
    if reconstructed
        && (projection.generation().get() < 2 || !completed_runtime.resumed.contains(&4))
    {
        return Err(invalid("simulated task did not resume its durable checkpoint").into());
    }
    let mut bytes = [0; 64];
    let length = completed_journal.read_blob(binding.scope(), output, &mut bytes)?;
    let bytes = bytes
        .get(..length)
        .ok_or_else(|| invalid("invalid output length"))?;
    let verified = VerifiedTaskEffectOutcome::verify(
        &binding,
        &projection,
        projection.generation(),
        bytes,
        &Sum,
        None,
    )?;
    let relay = TaskResultRelay::new(verified)?;
    let result_input = relay.linear_input(None);
    let result_event = result_input.to_event(&saga);
    let finished = decide(
        &saga,
        Some(&running.projection),
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        &result_input,
    )?;
    let result_digest = persist(&store, &scope, &result_event)?;
    let result = batch(
        &finished,
        &result_event,
        &scope,
        relay.input().clone(),
        sequence,
        result_digest,
    )?;
    wait(relay_task_result(
        &policy,
        &policy.principal,
        &relay,
        &store,
        &result,
    ))?;
    let (terminal_events, _sequence) = events(&store, &scope)?;
    if penelope_executor::engine::replay(
        &saga,
        &scope.tenant_id,
        &scope.process_id,
        &terminal_events,
    )?
    .projection
    .status
        != SagaStatus::Completed
    {
        return Err(invalid("durable saga replay did not complete").into());
    }
    println!("Verified sum: 6; simulated task completed the durable saga");
    Ok(())
}
