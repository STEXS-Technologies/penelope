//! One model across memory, durable recovery and native task/saga admission.
use async_trait::async_trait;
use penelope::ports::PortError;
use penelope::saga::{
    LinearSagaDefinition, MachineSagaDefinition, MachineSagaReceiver, ProcessAuthorizationDecision,
    ProcessAuthorizationOperation, ProcessAuthorizationRequest, ProcessAuthorizer, RetryPolicy,
    StepPlan,
};
use penelope::task::{
    Digest, EffectContract, ExecutionBudget, MachineTaskReceiver, RecoveryMode, TaskDefinition,
    TaskRecoveryPolicy, TaskSequence, TaskTick,
};
use penelope::{ContentDigest, LogicalTime, ProcessActionKind};
use penelope::{machine::*, storage::machine::*};
use penelope_sqlite::{
    SqliteLeaseClock, SqliteLimits, SqliteMachineOutbox, SqliteMachineStore, SqliteProcessStore,
    SqliteTaskJournal,
};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};
#[path = "../../crates/penelope/examples/support/counter.rs"]
mod counter;
#[path = "../../crates/penelope/examples/support/counter_codec.rs"]
mod counter_codec;
use counter::{Counter, DEFINITION};
use counter_codec::CounterCodec;
#[cfg(feature = "protected")]
mod workflow;
const SCOPE: MachineScope = MachineScope {
    tenant: MachineTenantId::from_bytes([1; 32]),
    instance: MachineId::from_bytes([2; 32]),
    definition: DEFINITION,
};
const PAYLOADS: MachinePayloadLimits = MachinePayloadLimits {
    max_record_bytes: 4096,
    max_state_bytes: 8,
    max_event_bytes: 8,
    max_intent_bytes: 8,
    max_batch_bytes: 1024,
};
const INTENTS: IntentLimits = IntentLimits {
    max_intents: 4,
    max_payload_bytes: 32,
};
struct LocalOwner;
#[async_trait]
impl MachineAuthorizer for LocalOwner {
    // This local program selects its trusted owner. A service must derive this
    // context from its authenticated host request, never the submitted payload.
    type Context = MachineTenantId;
    async fn authorize(
        &self,
        owner: &MachineTenantId,
        scope: MachineScope,
        _operation: MachineOperation,
    ) -> Result<(), MachineStoreError> {
        if *owner == SCOPE.tenant && scope.tenant == *owner {
            Ok(())
        } else {
            Err(MachineStoreError::Denied)
        }
    }
}
struct LocalClock(AtomicU64);
impl LocalClock {
    fn new() -> Self {
        Self(AtomicU64::new(100))
    }
    fn advance(&self) {
        self.0.store(111, Ordering::SeqCst);
    }
}
impl MachineLeaseClock for LocalClock {
    fn now(&self) -> Result<MachineLeaseTick, MachineStoreError> {
        Ok(MachineLeaseTick::new(self.0.load(Ordering::SeqCst)))
    }
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::park();
    }
}
fn commit_counter(
    store: &SqliteMachineStore<LocalOwner, 4>,
    machine: &mut MemoryMachine<Counter, 4>,
    owner: &MachineTenantId,
    event: u64,
    event_id: MachineEventId,
    logical_time: u64,
    causation: Option<MachineCausation>,
) -> Result<Arc<MachineCommitBatch<4>>, Box<dyn std::error::Error>> {
    let context = TransitionContext {
        expected: machine.read().0,
        event: event_id,
        event_digest: penelope::core::task::Digest::of(&event.to_le_bytes()).0,
        logical_time: MachineLogicalTime::new(logical_time),
    };
    let mut state = [0; 8];
    let mut encoded_event = [0; 8];
    let mut intent = [0; 8];
    let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
        machine,
        &MachineCommitInput {
            event: &event,
            context,
            codec: &CounterCodec,
            causation,
        },
        &mut MachineCommitBuffers {
            state: &mut state,
            event: &mut encoded_event,
            intent: &mut intent,
        },
        PAYLOADS,
    )?
    else {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    };
    let retained = prepared.batch();
    let receipt = block_on(store.commit(owner, Arc::clone(&retained)))?;
    prepared.publish(receipt)?;
    let _ = machine.take_intents();
    Ok(retained)
}
fn claim(
    outbox: &impl MachineIntentOutbox<Context = MachineTenantId>,
    owner: &MachineTenantId,
) -> Result<MachineIntentLease, Box<dyn std::error::Error>> {
    block_on(outbox.claim(
        owner,
        MachineIntentClaim {
            scope: SCOPE,
            owner: MachineLeaseOwner::from_bytes([9; 32]),
            max_intents: 1,
            max_bytes: 512,
        },
    ))?
    .pop()
    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound).into())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("counter.db");
    let store =
        SqliteMachineStore::<_, 4>::open(&path, SqliteLimits::default(), PAYLOADS, LocalOwner, 8)?;
    let owner = SCOPE.tenant;
    let bootstrap =
        prepare_machine_bootstrap(&Counter, &CounterCodec, SCOPE, &0, &mut [0; 8], PAYLOADS)?;
    block_on(store.bootstrap(&owner, Arc::new(bootstrap)))?;
    let snapshot = block_on(store.load_head(&owner, SCOPE))?
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
    let mut machine: MemoryMachine<_, 4> =
        snapshot.restore(Counter, &CounterCodec, SCOPE, &mut [0; 8], INTENTS)?;
    let retained = commit_counter(
        &store,
        &mut machine,
        &owner,
        256,
        MachineEventId::from_bytes([4; 32]),
        1,
        None,
    )?;
    println!(
        "committed={} revision={}",
        machine.read().1,
        machine.read().0.revision.get()
    );
    let before = machine.read().0;
    let overflow = u64::MAX;
    let rejected = matches!(
        machine.prepare(
            &overflow,
            TransitionContext {
                expected: before,
                event: MachineEventId::from_bytes([6; 32]),
                event_digest: Digest::of(&overflow.to_le_bytes()).0,
                logical_time: MachineLogicalTime::new(2),
            }
        )?,
        PrepareDecision::Rejected { .. }
    );
    if !rejected || machine.read().0 != before || *machine.read().1 != 256 {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    }
    println!("overflow_rejected=true revision_unchanged=true");
    drop(store);
    let reopened =
        SqliteMachineStore::<_, 4>::open(&path, SqliteLimits::default(), PAYLOADS, LocalOwner, 8)?;
    let duplicate = block_on(reopened.commit(&owner, retained))?;
    let stored = block_on(reopened.lookup(&owner, SCOPE, MachineEventId::from_bytes([4; 32])))?
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
    if duplicate != stored.receipt() {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    }
    let mut restored = stored.restore(Counter, &CounterCodec, SCOPE, &mut [0; 8], INTENTS)?;
    println!(
        "restored={} retained_intents={} duplicate={:?}",
        restored.read().1,
        stored.batch().intents().count(),
        duplicate.disposition
    );
    // The same committed machine intent becomes one durable native task input.
    // The default profile demonstrates admission. The protected profile below
    // supplies the production Linux runtime to complete this pure task.
    let definition = TaskDefinition {
        code: Digest::of(b"counter-task-admission-example-v1"),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromBeginning,
            max_attempts: 2,
            timeout_ticks: 10,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 8,
        },
        budget: ExecutionBudget {
            max_input_bytes: 8,
            max_output_bytes: 8,
            max_history_records: 32,
            max_checkpoint_commits: 4,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(1000),
        },
    };
    #[cfg(feature = "protected")]
    let (definition, worker) = workflow::pin_worker(definition)?;
    let tasks = Arc::new(
        SqliteTaskJournal::open(
            &directory.path().join("tasks.db"),
            definition,
            SqliteLimits::default(),
        )?
        .into_async(8)?,
    );
    let receiver = Arc::new(MachineTaskReceiver::new(
        tasks,
        MachineSourceId::from_bytes([8; 32]),
        definition,
        1,
    )?);
    let journal = Arc::new(reopened);
    let clock = Arc::new(LocalClock::new());
    let outbox = SqliteMachineOutbox::new(
        Arc::clone(&journal),
        Arc::clone(&clock),
        Arc::clone(&receiver),
        MachineOutboxLimits {
            lease_ticks: 10,
            max_claims: 1,
            max_claim_bytes: 512,
            max_attempts: 3,
        },
    )?;
    let lease = claim(&outbox, &owner)?;
    let accepted = block_on(receiver.accept(&outbox, &owner, &lease, TaskTick::new(1)))?;
    // Simulate losing the reply after native commit and exiting before source ACK.
    // Reopen both databases, reclaim only after expiry, and resolve native history.
    let source = lease.metadata().source;
    let provenance = stored.receipt();
    if source.head != provenance.head
        || source.event != provenance.event
        || source.commit != provenance.identity
    {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    }
    println!(
        "provenance_verified=true source_revision={}",
        source.head.revision.get()
    );
    drop(outbox);
    drop(receiver);
    drop(journal);
    let journal = Arc::new(SqliteMachineStore::<_, 4>::open(
        &path,
        SqliteLimits::default(),
        PAYLOADS,
        LocalOwner,
        8,
    )?);
    let tasks = Arc::new(
        SqliteTaskJournal::open(
            &directory.path().join("tasks.db"),
            definition,
            SqliteLimits::default(),
        )?
        .into_async(8)?,
    );
    let receiver = Arc::new(MachineTaskReceiver::new(
        tasks,
        MachineSourceId::from_bytes([8; 32]),
        definition,
        1,
    )?);
    clock.advance();
    let outbox = SqliteMachineOutbox::new(
        Arc::clone(&journal),
        Arc::clone(&clock),
        Arc::clone(&receiver),
        MachineOutboxLimits {
            lease_ticks: 10,
            max_claims: 1,
            max_claim_bytes: 512,
            max_attempts: 3,
        },
    )?;
    let recovered_lease = claim(&outbox, &owner)?;
    let recovered = block_on(receiver.accept(&outbox, &owner, &recovered_lease, TaskTick::new(2)))?;
    if recovered != accepted
        || recovered_lease.metadata().source != source
        || recovered_lease.metadata().attempt.get() != 2
        || !matches!(
            block_on(outbox.acknowledge(&owner, &lease, &accepted)),
            Err(MachineStoreError::Conflict)
        )
    {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    }
    println!("reopened=true task_recovered=true attempt=2 stale_ack_refused=true");
    let lease = recovered_lease;
    let acknowledgement = block_on(outbox.acknowledge(&owner, &lease, &accepted))?;
    if accepted.sequence != TaskSequence::ZERO || acknowledgement.source != lease.metadata().source
    {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    }
    println!(
        "task_admitted=true generation={} source_acknowledged=true",
        accepted.generation.get()
    );
    // Task admission and source acknowledgement do not establish task completion.
    // The optional Linux progression executes through the native supervisor and
    // admits only the verified durable result as the next application event.
    #[cfg(feature = "protected")]
    let completed = {
        let binding = receiver.binding(source, lease.payload())?;
        drop(outbox);
        drop(receiver);
        workflow::complete(directory.path(), &worker, definition, binding)?
    };
    #[cfg(feature = "protected")]
    let (amount, event_id, causation) =
        (completed.amount, completed.event, Some(completed.causation));
    #[cfg(not(feature = "protected"))]
    let (amount, event_id, causation) = (1, MachineEventId::from_bytes([5; 32]), None);
    // A second committed event starts one host-pinned saga. Its action remains
    // in the native outbox; this example does not dispatch external effects.
    let result = commit_counter(
        &journal,
        &mut restored,
        &owner,
        amount,
        event_id,
        2,
        causation,
    )?;
    let result_receipt = block_on(journal.commit(&owner, Arc::clone(&result)))?;
    if result_receipt.disposition != MachineCommitDisposition::AlreadyCommitted
        || result.record().header().causation != causation
    {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData).into());
    }
    #[cfg(feature = "protected")]
    println!(
        "result_committed=true state={} revision={}",
        restored.read().1,
        restored.read().0.revision.get()
    );
    let processes = Arc::new(SqliteProcessStore::open(
        &directory.path().join("sagas.db"),
        SqliteLimits::default(),
        clock.clone(),
        10,
        8,
    )?);
    let policy = MachineSagaDefinition::Linear(LinearSagaDefinition {
        definition_id: "def_counter".parse()?,
        definition_version: "dfv_one".parse()?,
        definition_digest: ContentDigest::sha256(b"counter-workflow-example-v1"),
        steps: vec![StepPlan {
            step_id: "stp_notify".parse()?,
            action_kind: ProcessActionKind::ExternalEffect,
            payload_digest: ContentDigest::sha256(b"host-defined-notification"),
            retry_policy: RetryPolicy::no_retry(),
            compensation: None,
        }],
    });
    let saga = Arc::new(MachineSagaReceiver::new(
        processes,
        Arc::new(LocalSagaOwner),
        MachineSourceId::from_bytes([10; 32]),
        "pri_local".parse()?,
        policy,
        8,
    )?);
    let saga_outbox = SqliteMachineOutbox::new(
        journal,
        Arc::clone(&clock),
        Arc::clone(&saga),
        MachineOutboxLimits {
            lease_ticks: 10,
            max_claims: 1,
            max_claim_bytes: 512,
            max_attempts: 3,
        },
    )?;
    let lease = claim(&saga_outbox, &owner)?;
    let receipt = block_on(saga.accept(&saga_outbox, &owner, &lease, LogicalTime(1)))?;
    block_on(saga_outbox.acknowledge(&owner, &lease, &receipt))?;
    println!(
        "saga_admitted=true native_actions={} source_acknowledged=true",
        receipt.commit().actions.len()
    );
    Ok(())
}

impl SqliteLeaseClock for LocalClock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0.load(Ordering::SeqCst)))
    }
}
struct LocalSagaOwner;
#[async_trait]
impl ProcessAuthorizer for LocalSagaOwner {
    async fn authorize(
        &self,
        request: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        // This standalone demo pins a local service identity; hosted applications
        // inject authenticated principals and their exact process-scope policy.
        Ok(
            if request.principal_id.as_str() == "pri_local"
                && request.scope.definition_id.as_str() == "def_counter"
                && request.operation == ProcessAuthorizationOperation::Start
            {
                ProcessAuthorizationDecision::Authorized
            } else {
                ProcessAuthorizationDecision::Denied
            },
        )
    }
}
