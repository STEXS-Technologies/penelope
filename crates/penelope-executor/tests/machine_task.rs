//! Real machine journal/outbox and native task journal; no alternate task engine.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
use async_trait::async_trait;
use penelope_core::{
    machine::*,
    task::{runtime::TaskPortError, *},
};
use penelope_executor::{
    machine_task::{MachineTaskHandoffError, MachineTaskReceiver},
    task::TaskSupervisor,
};
use penelope_ports::{machine::*, tasks::AsyncTaskJournal};
use penelope_sqlite::{
    SqliteAsyncTaskJournal, SqliteLimits, SqliteMachineOutbox, SqliteMachineStore,
    SqliteTaskJournal,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
#[path = "../../penelope-sqlite/tests/support/machines.rs"]
#[allow(dead_code)]
mod machine;
#[path = "support/tasks.rs"]
mod native;
use machine::{SCOPE, block_on};
struct Clock(AtomicU64);
impl MachineLeaseClock for Clock {
    fn now(&self) -> Result<MachineLeaseTick, MachineStoreError> {
        Ok(MachineLeaseTick::new(self.0.load(Ordering::SeqCst)))
    }
}
const NAMESPACE: MachineSourceId = MachineSourceId::from_bytes([7; 32]);
const OUTBOX: MachineOutboxLimits = MachineOutboxLimits {
    lease_ticks: 10,
    max_claims: 1,
    max_claim_bytes: 512,
    max_attempts: 4,
};
const fn claim() -> MachineIntentClaim {
    MachineIntentClaim {
        scope: SCOPE,
        owner: MachineLeaseOwner::from_bytes([9; 32]),
        max_intents: 1,
        max_bytes: 512,
    }
}
fn definition() -> TaskDefinition {
    native::definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure)
}
fn source(path: &std::path::Path) -> Arc<SqliteMachineStore<machine::Policy, 2>> {
    let store = Arc::new(machine::open(path));
    block_on(store.bootstrap(&true, machine::genesis())).unwrap();
    block_on(store.commit(&true, machine::batch(&mut machine::owner(), 8, 4))).unwrap();
    store
}
fn journal(path: &std::path::Path) -> Arc<SqliteAsyncTaskJournal> {
    journal_with_definition(path, definition())
}
fn journal_with_definition(
    path: &std::path::Path,
    policy: TaskDefinition,
) -> Arc<SqliteAsyncTaskJournal> {
    Arc::new(
        SqliteTaskJournal::open(path, policy, SqliteLimits::default())
            .unwrap()
            .into_async(8)
            .unwrap(),
    )
}
#[test]
fn shared_and_separate_stores_reconstruct_native_acceptance_after_lost_ack() {
    for shared in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let machine_path = directory.path().join("machine.db");
        let task_path = if shared {
            machine_path.clone()
        } else {
            directory.path().join("tasks.db")
        };
        let store = source(&machine_path);
        let tasks = journal(&task_path);
        let receiver = Arc::new(
            MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition(), 1).unwrap(),
        );
        let clock = Arc::new(Clock(AtomicU64::new(100)));
        let outbox = SqliteMachineOutbox::new(
            Arc::clone(&store),
            Arc::clone(&clock),
            Arc::clone(&receiver),
            OUTBOX,
        )
        .unwrap();
        let first = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
        let accepted = block_on(receiver.accept(&outbox, &true, &first, TaskTick::new(1))).unwrap();
        assert_eq!(accepted.sequence, TaskSequence::ZERO);
        assert_eq!(accepted.generation, TaskGeneration::ZERO);
        let binding = receiver
            .binding(first.metadata().source, first.payload())
            .unwrap();
        binding.validate(&accepted).unwrap();
        assert_eq!(
            block_on(AsyncTaskJournal::read(
                &*tasks,
                binding.scope(),
                TaskSequence::new(1)
            ))
            .unwrap(),
            None
        );
        // Source ACK was never sent. Reopen, expire and reconcile using the same native intent.
        drop(outbox);
        drop(receiver);
        drop(tasks);
        drop(store);
        clock.0.store(110, Ordering::SeqCst);
        let reopened_source = Arc::new(machine::open(&machine_path));
        let reopened_tasks = journal(&task_path);
        let reopened_receiver = Arc::new(
            MachineTaskReceiver::new(Arc::clone(&reopened_tasks), NAMESPACE, definition(), 1)
                .unwrap(),
        );
        let reopened_outbox = SqliteMachineOutbox::new(
            reopened_source,
            clock,
            Arc::clone(&reopened_receiver),
            OUTBOX,
        )
        .unwrap();
        let retry = block_on(reopened_outbox.claim(&true, claim()))
            .unwrap()
            .remove(0);
        assert!(retry.requires_reconciliation());
        assert_eq!(
            block_on(reopened_receiver.accept(&reopened_outbox, &true, &retry, TaskTick::new(50)))
                .unwrap(),
            accepted
        );
        let ack = block_on(reopened_outbox.acknowledge(&true, &retry, &accepted)).unwrap();
        assert_eq!(ack.source, first.metadata().source);
        assert_eq!(
            ack.receipt_digest,
            binding.receipt_digest(&accepted).unwrap()
        );
        assert_eq!(
            block_on(reopened_outbox.status(&true, SCOPE, ack.source.intent.identity))
                .unwrap()
                .unwrap()
                .accepted,
            Some(ack)
        );
        assert!(
            block_on(reopened_outbox.claim(&true, claim()))
                .unwrap()
                .is_empty()
        );
        // A native supervisor can recover and run independently after source acceptance.
        let native_journal =
            SqliteTaskJournal::open(&task_path, definition(), SqliteLimits::default()).unwrap();
        let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
            native_journal,
            native::SumRuntime::default(),
            binding.scope(),
            definition(),
            TaskTick::new(51),
        )
        .unwrap();
        assert_eq!(supervisor.projection().status(), TaskStatus::Pending);
        supervisor.advance(TaskTick::new(52)).unwrap();
        assert_eq!(supervisor.projection().generation().get(), 1);
    }
}
#[test]
fn redelivery_of_running_or_faulted_task_never_submits_a_second_intent_or_generation() {
    for (fault, effects) in [
        (false, EffectContract::Pure),
        (true, EffectContract::Pure),
        (true, EffectContract::ReconcileRequired),
    ] {
        let task_policy = native::definition(RecoveryMode::RestartFromBeginning, effects);
        let directory = tempfile::tempdir().unwrap();
        let store = source(&directory.path().join("machine.db"));
        let path = directory.path().join("tasks.db");
        let tasks = journal_with_definition(&path, task_policy);
        let receiver = Arc::new(
            MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, task_policy, 1).unwrap(),
        );
        let clock = Arc::new(Clock(AtomicU64::new(100)));
        let outbox =
            SqliteMachineOutbox::new(store, Arc::clone(&clock), Arc::clone(&receiver), OUTBOX)
                .unwrap();
        let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
        let accepted = block_on(receiver.accept(&outbox, &true, &lease, TaskTick::new(1))).unwrap();
        let mut projection = TaskProjection::new(accepted.scope, task_policy).unwrap();
        projection.apply(&accepted).unwrap();
        let start = projection.next(TaskTick::new(2)).unwrap().unwrap();
        block_on(AsyncTaskJournal::commit(&*tasks, &start, None)).unwrap();
        projection.apply(&start).unwrap();
        if fault {
            let record = projection
                .propose(
                    TaskExecutionEvent::FaultObserved {
                        fault: TaskFault::SupervisorInterrupted,
                    },
                    projection.generation(),
                    TaskTick::new(3),
                )
                .unwrap();
            block_on(AsyncTaskJournal::commit(&*tasks, &record, None)).unwrap();
            projection.apply(&record).unwrap();
        }
        clock.0.store(110, Ordering::SeqCst);
        let retry = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
        assert_eq!(
            block_on(receiver.accept(&outbox, &true, &retry, TaskTick::new(20))).unwrap(),
            accepted
        );
        assert_eq!(
            block_on(AsyncTaskJournal::read(
                &*tasks,
                accepted.scope,
                projection.next_sequence()
            ))
            .unwrap(),
            None
        );
        block_on(outbox.acknowledge(&true, &retry, &accepted)).unwrap();
        assert_eq!(projection.generation().get(), 1);
    }
}
#[test]
fn receipt_schema_scope_input_and_tick_substitution_never_acknowledge_the_source() {
    let directory = tempfile::tempdir().unwrap();
    let store = source(&directory.path().join("machine.db"));
    let tasks = journal(&directory.path().join("tasks.db"));
    let receiver =
        Arc::new(MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition(), 1).unwrap());
    let outbox = SqliteMachineOutbox::new(
        store,
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        OUTBOX,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    let accepted = block_on(receiver.accept(&outbox, &true, &lease, TaskTick::new(1))).unwrap();
    for change in 0..5 {
        let mut forged = accepted;
        match change {
            0 => forged.tick = TaskTick::new(2),
            1 => forged.scope.task = TaskId::from_digest(Digest([0; 32])),
            2 => forged.generation = TaskGeneration::new(1),
            3 => {
                let TaskExecutionEvent::IntentCommitted {
                    definition,
                    mut input,
                } = forged.event
                else {
                    panic!("intent")
                };
                input.schema = 2;
                forged.event = TaskExecutionEvent::IntentCommitted { definition, input };
            }
            _ => {
                let TaskExecutionEvent::IntentCommitted {
                    definition,
                    mut input,
                } = forged.event
                else {
                    panic!("intent")
                };
                input.digest = Digest([0; 32]);
                forged.event = TaskExecutionEvent::IntentCommitted { definition, input };
            }
        }
        assert!(block_on(outbox.acknowledge(&true, &lease, &forged)).is_err());
        assert_eq!(
            block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
                .unwrap()
                .unwrap()
                .accepted,
            None
        );
    }
    block_on(outbox.acknowledge(&true, &lease, &accepted)).unwrap();
}
#[test]
fn receiver_policy_identity_changes_with_definition_schema_and_namespace_before_admission() {
    let directory = tempfile::tempdir().unwrap();
    let store = source(&directory.path().join("machine.db"));
    let tasks = journal(&directory.path().join("tasks.db"));
    let receiver =
        Arc::new(MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition(), 1).unwrap());
    let outbox = SqliteMachineOutbox::new(
        store,
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        OUTBOX,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    for change in 0..3 {
        let mut policy = definition();
        if change == 0 {
            policy.effects = EffectContract::ReconcileRequired;
        }
        let other = MachineTaskReceiver::new(
            Arc::clone(&tasks),
            if change == 1 {
                MachineSourceId::from_bytes([6; 32])
            } else {
                NAMESPACE
            },
            policy,
            if change == 2 { 2 } else { 1 },
        )
        .unwrap();
        assert!(matches!(
            block_on(other.accept(&outbox, &true, &lease, TaskTick::new(1))),
            Err(MachineTaskHandoffError::Source(MachineStoreError::Conflict))
        ));
    }
    let binding = receiver
        .binding(lease.metadata().source, lease.payload())
        .unwrap();
    assert_eq!(
        block_on(AsyncTaskJournal::read(
            &*tasks,
            binding.scope(),
            TaskSequence::ZERO
        ))
        .unwrap(),
        None
    );
}
/// Real journal with an injected native ACK loss; the underlying durable path is unchanged.
struct LostAck {
    inner: Arc<SqliteAsyncTaskJournal>,
    lose: AtomicBool,
}
#[async_trait]
impl AsyncTaskJournal for LostAck {
    async fn read(
        &self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        self.inner.read(scope, sequence).await
    }
    async fn commit(
        &self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        let accepted = self.inner.commit(record, blob).await?;
        if self.lose.swap(false, Ordering::SeqCst) {
            Err(TaskPortError::Unavailable)
        } else {
            Ok(accepted)
        }
    }
    async fn read_blob(
        &self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        self.inner.read_blob(scope, blob, buffer).await
    }
}
#[test]
fn lost_native_commit_ack_leaves_pending_source_and_reconstructs_the_original_intent() {
    let directory = tempfile::tempdir().unwrap();
    let store = source(&directory.path().join("machine.db"));
    let tasks = Arc::new(LostAck {
        inner: journal(&directory.path().join("tasks.db")),
        lose: AtomicBool::new(true),
    });
    let receiver =
        Arc::new(MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition(), 1).unwrap());
    let outbox = SqliteMachineOutbox::new(
        store,
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        OUTBOX,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    assert!(matches!(
        block_on(receiver.accept(&outbox, &true, &lease, TaskTick::new(1))),
        Err(MachineTaskHandoffError::Task(TaskPortError::Unavailable))
    ));
    assert_eq!(
        block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
            .unwrap()
            .unwrap()
            .accepted,
        None
    );
    let accepted = block_on(receiver.accept(&outbox, &true, &lease, TaskTick::new(2))).unwrap();
    assert_eq!(accepted.tick, TaskTick::new(1));
    block_on(outbox.acknowledge(&true, &lease, &accepted)).unwrap();
}

struct TargetPolicy(MachineSourceId);
#[async_trait]
impl MachineAuthorizer for TargetPolicy {
    type Context = bool;
    async fn authorize(
        &self,
        authenticated: &bool,
        scope: MachineScope,
        operation: MachineOperation,
    ) -> Result<(), MachineStoreError> {
        if !authenticated || scope != SCOPE {
            return Err(MachineStoreError::Denied);
        }
        if let MachineOperation::ValidateIntent { consumer, .. }
        | MachineOperation::AcknowledgeIntent { consumer, .. } = operation
            && consumer != self.0
        {
            return Err(MachineStoreError::Denied);
        }
        Ok(())
    }
}
#[test]
fn target_specific_authorization_precedes_any_native_admission() {
    let directory = tempfile::tempdir().unwrap();
    let tasks = journal(&directory.path().join("tasks.db"));
    let receiver =
        Arc::new(MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition(), 1).unwrap());
    let store = Arc::new(
        SqliteMachineStore::<_, 2>::open(
            &directory.path().join("machine.db"),
            SqliteLimits::default(),
            machine::PAYLOADS,
            TargetPolicy(MachineSourceId::from_bytes([0; 32])),
            8,
        )
        .unwrap(),
    );
    block_on(store.bootstrap(&true, machine::genesis())).unwrap();
    block_on(store.commit(&true, machine::batch(&mut machine::owner(), 8, 4))).unwrap();
    let outbox = SqliteMachineOutbox::new(
        store,
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        OUTBOX,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    assert_eq!(
        block_on(receiver.accept(&outbox, &true, &lease, TaskTick::new(1))),
        Err(MachineTaskHandoffError::Source(MachineStoreError::Denied))
    );
    let binding = receiver
        .binding(lease.metadata().source, lease.payload())
        .unwrap();
    assert_eq!(
        block_on(AsyncTaskJournal::read(
            &*tasks,
            binding.scope(),
            TaskSequence::ZERO
        ))
        .unwrap(),
        None
    );
}
#[test]
fn independent_native_writers_reconstruct_one_exact_receipt_after_racing_admission() {
    let directory = tempfile::tempdir().unwrap();
    let store = source(&directory.path().join("machine.db"));
    let path = directory.path().join("tasks.db");
    let receiver =
        Arc::new(MachineTaskReceiver::new(journal(&path), NAMESPACE, definition(), 1).unwrap());
    let other = MachineTaskReceiver::new(journal(&path), NAMESPACE, definition(), 1).unwrap();
    let outbox = Arc::new(
        SqliteMachineOutbox::new(
            store,
            Arc::new(Clock(AtomicU64::new(100))),
            Arc::clone(&receiver),
            OUTBOX,
        )
        .unwrap(),
    );
    let lease = Arc::new(block_on(outbox.claim(&true, claim())).unwrap().remove(0));
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let second_barrier = Arc::clone(&barrier);
    let second_outbox = Arc::clone(&outbox);
    let second_lease = Arc::clone(&lease);
    let first_receiver = Arc::clone(&receiver);
    let a = std::thread::spawn(move || {
        barrier.wait();
        block_on(first_receiver.accept(&*outbox, &true, &lease, TaskTick::new(1))).unwrap()
    });
    let result = std::thread::spawn(move || {
        second_barrier.wait();
        block_on(other.accept(&*second_outbox, &true, &second_lease, TaskTick::new(2))).unwrap()
    });
    assert_eq!(a.join().unwrap(), result.join().unwrap());
    let connection = rusqlite::Connection::open(path).unwrap();
    let records: u32 = connection
        .query_row("SELECT count(*) FROM task_records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(records, 1);
}
#[test]
fn damaged_native_blob_never_becomes_verified_receiver_acceptance() {
    let directory = tempfile::tempdir().unwrap();
    let store = source(&directory.path().join("machine.db"));
    let path = directory.path().join("tasks.db");
    let receiver =
        Arc::new(MachineTaskReceiver::new(journal(&path), NAMESPACE, definition(), 1).unwrap());
    let outbox = SqliteMachineOutbox::new(
        store,
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        OUTBOX,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    let accepted = block_on(receiver.accept(&outbox, &true, &lease, TaskTick::new(1))).unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute("UPDATE task_blobs SET bytes=zeroblob(8)", [])
        .unwrap();
    assert_eq!(
        block_on(outbox.acknowledge(&true, &lease, &accepted)),
        Err(MachineStoreError::Corrupt)
    );
    assert_eq!(
        block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
            .unwrap()
            .unwrap()
            .accepted,
        None
    );
}
#[cfg(unix)]
#[path = "support/machine_task_crash.rs"]
mod crash;

#[test]
fn native_scope_quota_refuses_admission_without_acknowledging_source() {
    let directory = tempfile::tempdir().unwrap();
    let store = source(&directory.path().join("machine.db"));
    let tasks = Arc::new(
        SqliteTaskJournal::open(
            &directory.path().join("tasks.db"),
            definition(),
            SqliteLimits {
                max_scope_bytes: 1,
                ..SqliteLimits::default()
            },
        )
        .unwrap()
        .into_async(8)
        .unwrap(),
    );
    let receiver =
        Arc::new(MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition(), 1).unwrap());
    let outbox = SqliteMachineOutbox::new(
        store,
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        OUTBOX,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    assert_eq!(
        block_on(receiver.accept(&outbox, &true, &lease, TaskTick::new(1))),
        Err(MachineTaskHandoffError::Task(TaskPortError::Exhausted)),
    );
    let binding = receiver
        .binding(lease.metadata().source, lease.payload())
        .unwrap();
    assert_eq!(
        block_on(tasks.read(binding.scope(), TaskSequence::ZERO)).unwrap(),
        None
    );
    assert_eq!(
        block_on(outbox.status(&true, SCOPE, lease.metadata().source.intent.identity))
            .unwrap()
            .unwrap()
            .accepted,
        None,
    );
}

#[test]
fn reopened_outbox_with_changed_receiver_cannot_admit_a_second_task_after_lost_source_ack() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let store = source(&path);
    let tasks = journal(&directory.path().join("tasks.db"));
    let original =
        Arc::new(MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition(), 1).unwrap());
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let outbox = SqliteMachineOutbox::new(
        Arc::clone(&store),
        Arc::clone(&clock),
        Arc::clone(&original),
        OUTBOX,
    )
    .unwrap();
    let first = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    let native = block_on(original.accept(&outbox, &true, &first, TaskTick::new(1))).unwrap();
    drop(outbox);
    drop(store);
    let reopened_store = Arc::new(machine::open(&path));
    let changed = Arc::new(
        MachineTaskReceiver::new(
            Arc::clone(&tasks),
            MachineSourceId::from_bytes([99; 32]),
            definition(),
            1,
        )
        .unwrap(),
    );
    let other = SqliteMachineOutbox::new(
        Arc::clone(&reopened_store),
        Arc::clone(&clock),
        Arc::clone(&changed),
        OUTBOX,
    )
    .unwrap();
    assert!(matches!(
        block_on(changed.accept(&other, &true, &first, TaskTick::new(2))),
        Err(MachineTaskHandoffError::Source(MachineStoreError::Conflict))
    ));
    let changed_scope = changed
        .binding(first.metadata().source, first.payload())
        .unwrap()
        .scope();
    assert_eq!(
        block_on(tasks.read(changed_scope, TaskSequence::ZERO)).unwrap(),
        None
    );
    clock.0.store(110, Ordering::SeqCst);
    assert!(matches!(
        block_on(other.claim(&true, claim())),
        Err(MachineStoreError::Conflict)
    ));
    let resumed =
        SqliteMachineOutbox::new(reopened_store, clock, Arc::clone(&original), OUTBOX).unwrap();
    let reclaimed = block_on(resumed.claim(&true, claim())).unwrap().remove(0);
    assert_eq!(
        block_on(original.accept(&resumed, &true, &reclaimed, TaskTick::new(3))).unwrap(),
        native
    );
    block_on(resumed.acknowledge(&true, &reclaimed, &native)).unwrap();
}
