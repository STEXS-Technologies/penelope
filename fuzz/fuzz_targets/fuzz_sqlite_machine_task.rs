#![no_main]
//! Real source/native journals against an independent handoff/delivery-count oracle.
use libfuzzer_sys::fuzz_target;
use penelope_core::{machine::*, task::*};
use penelope_executor::machine_task::{MachineTaskHandoffError, MachineTaskReceiver};
use penelope_ports::{machine::*, tasks::AsyncTaskJournal};
use penelope_sqlite::{SqliteLimits, SqliteMachineOutbox, SqliteMachineStore, SqliteTaskJournal};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[path = "../../crates/penelope-executor/tests/support/tasks.rs"]
mod native;
#[path = "../../crates/penelope-sqlite/tests/support/machines.rs"]
#[allow(dead_code)]
mod support;
use support::*;
struct Clock(AtomicU64);
impl MachineLeaseClock for Clock {
    fn now(&self) -> Result<MachineLeaseTick, MachineStoreError> {
        Ok(MachineLeaseTick::new(self.0.load(Ordering::SeqCst)))
    }
}
const LIMITS: MachineOutboxLimits = MachineOutboxLimits {
    lease_ticks: 10,
    max_claims: 1,
    max_claim_bytes: 512,
    max_attempts: 4,
};
const NAMESPACE: MachineSourceId = MachineSourceId::from_bytes([7; 32]);
fuzz_target!(|data: &[u8]| {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.db");
    let task_path = if data.first().is_some_and(|n| n & 1 == 0) {
        source_path.clone()
    } else {
        directory.path().join("tasks.db")
    };
    let definition = native::definition(
        RecoveryMode::RestartFromBeginning,
        if data.first().is_some_and(|n| n & 2 != 0) {
            EffectContract::ReconcileRequired
        } else {
            EffectContract::Pure
        },
    );
    let make_source = || Arc::new(open(&source_path));
    let make_task = || {
        Arc::new(
            SqliteTaskJournal::open(&task_path, definition, SqliteLimits::default())
                .unwrap()
                .into_async(4)
                .unwrap(),
        )
    };
    let mut source = make_source();
    block_on(source.bootstrap(&true, genesis())).unwrap();
    block_on(source.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let mut tasks = make_task();
    let mut receiver =
        Arc::new(MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition, 1).unwrap());
    let mut outbox = SqliteMachineOutbox::new(
        Arc::clone(&source),
        Arc::clone(&clock),
        Arc::clone(&receiver),
        LIMITS,
    )
    .unwrap();
    let claim = MachineIntentClaim {
        scope: SCOPE,
        owner: MachineLeaseOwner::from_bytes([9; 32]),
        max_intents: 1,
        max_bytes: 512,
    };
    let mut leases = block_on(outbox.claim(&true, claim)).unwrap();
    let binding = receiver
        .binding(leases[0].metadata().source, leases[0].payload())
        .unwrap();
    let mut tick = 100_u64;
    let mut attempt = 1_u64;
    let mut expires = 110_u64;
    let mut acknowledged = false;
    let mut native_receipt: Option<TaskRecord> = None;
    let mut record_count = 0_u32;
    for op in data.chunks(2).take(24) {
        let argument = op.get(1).copied().unwrap_or(0);
        let index = usize::from(argument) % leases.len();
        let lease = &leases[index];
        let current = lease.metadata().token.get() == attempt;
        let live = current && !acknowledged && tick < expires;
        match op[0] % 8 {
            0 => {
                let result = block_on(outbox.claim(&true, claim));
                let eligible = !acknowledged && tick >= expires;
                if eligible && attempt == 4 {
                    assert_eq!(result.err(), Some(MachineStoreError::Capacity));
                } else {
                    let mut claimed = result.unwrap();
                    assert_eq!(claimed.len(), usize::from(eligible));
                    if eligible {
                        attempt += 1;
                        expires = tick + 10;
                        let next = claimed.remove(0);
                        assert_eq!(next.metadata().token.get(), attempt);
                        assert!(next.requires_reconciliation());
                        leases.push(next);
                    }
                }
            }
            1 => {
                tick += u64::from(argument % 16);
                clock.0.store(tick, Ordering::SeqCst);
            }
            2 => {
                let result = block_on(receiver.accept(&outbox, &true, lease, TaskTick::new(1)));
                if live {
                    let accepted = result.unwrap();
                    if let Some(expected) = native_receipt {
                        assert_eq!(accepted, expected);
                    } else {
                        assert_eq!(accepted.scope, binding.scope());
                        assert_eq!(accepted.generation, TaskGeneration::ZERO);
                        assert_eq!(accepted.sequence, TaskSequence::ZERO);
                        native_receipt = Some(accepted);
                        record_count = 1;
                    }
                } else {
                    assert_eq!(
                        result,
                        Err(MachineTaskHandoffError::Source(MachineStoreError::Conflict))
                    );
                }
            }
            3 => {
                if let Some(receipt) = native_receipt {
                    let result = block_on(outbox.acknowledge(&true, lease, &receipt));
                    if current && (live || acknowledged) {
                        assert_eq!(result.unwrap().source, lease.metadata().source);
                        acknowledged = true;
                    } else {
                        assert_eq!(result, Err(MachineStoreError::Conflict));
                    }
                }
            }
            4 => {
                if native_receipt.is_some() && record_count < 3 {
                    let mut projection = TaskProjection::new(binding.scope(), definition).unwrap();
                    for sequence in 0..record_count {
                        projection
                            .apply(
                                &block_on(tasks.read(binding.scope(), TaskSequence::new(sequence)))
                                    .unwrap()
                                    .unwrap(),
                            )
                            .unwrap();
                    }
                    let record = if record_count == 1 {
                        projection.next(TaskTick::new(100)).unwrap().unwrap()
                    } else {
                        projection
                            .propose(
                                TaskExecutionEvent::FaultObserved {
                                    fault: TaskFault::SupervisorInterrupted,
                                },
                                TaskGeneration::new(1),
                                TaskTick::new(101),
                            )
                            .unwrap()
                    };
                    block_on(tasks.commit(&record, None)).unwrap();
                    record_count += 1;
                }
            }
            5 => {
                drop(outbox);
                drop(receiver);
                drop(tasks);
                drop(source);
                source = make_source();
                tasks = make_task();
                receiver = Arc::new(
                    MachineTaskReceiver::new(Arc::clone(&tasks), NAMESPACE, definition, 1).unwrap(),
                );
                outbox = SqliteMachineOutbox::new(
                    Arc::clone(&source),
                    Arc::clone(&clock),
                    Arc::clone(&receiver),
                    LIMITS,
                )
                .unwrap();
            }
            6 => {
                if let Some(mut receipt) = native_receipt {
                    receipt.generation = TaskGeneration::new(1);
                    assert!(block_on(outbox.acknowledge(&true, lease, &receipt)).is_err());
                }
            }
            _ => {
                assert_eq!(
                    block_on(receiver.accept(&outbox, &false, lease, TaskTick::new(1))),
                    Err(MachineTaskHandoffError::Source(MachineStoreError::Denied))
                );
            }
        }
        let status =
            block_on(outbox.status(&true, SCOPE, leases[0].metadata().source.intent.identity))
                .unwrap()
                .unwrap();
        assert_eq!(status.attempt.get(), attempt);
        assert_eq!(status.accepted.is_some(), acknowledged);
        assert_eq!(
            block_on(tasks.read(binding.scope(), TaskSequence::ZERO)).unwrap(),
            native_receipt
        );
        assert_eq!(
            block_on(tasks.read(binding.scope(), TaskSequence::new(record_count))).unwrap(),
            None
        );
    }
});
