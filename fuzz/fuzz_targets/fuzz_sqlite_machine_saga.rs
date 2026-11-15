#![no_main]
//! Real native/source journals against independent admission and delivery counts.
use async_trait::async_trait;
use libfuzzer_sys::fuzz_target;
use penelope_core::machine::*;
use penelope_domain::*;
use penelope_executor::{engine::*, machine_saga::*};
use penelope_ports::{machine::*, *};
use penelope_sqlite::{
    SqliteLeaseClock, SqliteLimits, SqliteMachineOutbox, SqliteMachineStore, SqliteProcessStore,
};
use std::{
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
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
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(self.0.load(Ordering::SeqCst)))
    }
}
struct NativePolicy(AtomicBool);
#[async_trait]
impl ProcessAuthorizer for NativePolicy {
    async fn authorize(
        &self,
        _request: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        Ok(if self.0.load(Ordering::SeqCst) {
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
            definition_id: id("def_fuzz"),
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
            definition_id: id("def_fuzz"),
            definition_version: id("dfv_one"),
            definition_digest: ContentDigest([3; 32]),
            steps,
        })
    }
}
const LIMITS: MachineOutboxLimits = MachineOutboxLimits {
    lease_ticks: 10,
    max_claims: 1,
    max_claim_bytes: 512,
    max_attempts: 4,
};
fn claim() -> MachineIntentClaim {
    MachineIntentClaim {
        scope: SCOPE,
        owner: MachineLeaseOwner::from_bytes([9; 32]),
        max_intents: 1,
        max_bytes: 512,
    }
}
fuzz_target!(|data: &[u8]| {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.db");
    let native_path = if data.first().is_some_and(|b| b & 1 == 0) {
        source_path.clone()
    } else {
        directory.path().join("native.db")
    };
    let graph = data.first().is_some_and(|b| b & 2 != 0);
    let make_source = || Arc::new(open(&source_path));
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let make_native = || {
        Arc::new(
            SqliteProcessStore::open(&native_path, SqliteLimits::default(), clock.clone(), 10, 8)
                .unwrap(),
        )
    };
    let policy = Arc::new(NativePolicy(AtomicBool::new(true)));
    let make_receiver = |store| {
        Arc::new(
            MachineSagaReceiver::new(
                store,
                policy.clone(),
                MachineSourceId::from_bytes([8; 32]),
                id("pri_fuzz"),
                definition(graph),
                64,
            )
            .unwrap(),
        )
    };
    let mut source = make_source();
    block_on(source.bootstrap(&true, genesis())).unwrap();
    block_on(source.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
    let mut native = make_native();
    let mut receiver = make_receiver(native.clone());
    let mut outbox =
        SqliteMachineOutbox::new(source.clone(), clock.clone(), receiver.clone(), LIMITS).unwrap();
    let first = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    let source_identity = first.metadata().source;
    let native_scope = receiver.scope(source_identity).unwrap();
    let mut leases = vec![first];
    let mut tick = 100_u64;
    let mut expiry = 110_u64;
    let mut attempts = 1_u64;
    let mut acknowledged = false;
    let mut receipt: Option<MachineSagaReceipt> = None;
    let mut authorized = true;
    for operation in data.chunks(2).take(24) {
        let argument = operation.get(1).copied().unwrap_or(0);
        let lease = &leases[usize::from(argument) % leases.len()];
        let current = lease.metadata().attempt.get() == attempts;
        let live = current && !acknowledged && tick < expiry;
        match operation[0] % 9 {
            0 => {
                let eligible = !acknowledged && tick >= expiry;
                let result = block_on(outbox.claim(&true, claim()));
                if eligible && attempts == 4 {
                    assert_eq!(result.err(), Some(MachineStoreError::Capacity));
                } else {
                    let mut result = result.unwrap();
                    assert_eq!(result.len(), usize::from(eligible));
                    if eligible {
                        attempts += 1;
                        expiry = tick + 10;
                        let lease = result.remove(0);
                        assert_eq!(lease.metadata().attempt.get(), attempts);
                        assert!(lease.requires_reconciliation());
                        leases.push(lease);
                    }
                }
            }
            1 => {
                tick += u64::from(argument % 16);
                clock.0.store(tick, Ordering::SeqCst);
            }
            2 => {
                let result = block_on(receiver.accept(
                    &outbox,
                    &true,
                    lease,
                    LogicalTime(u64::from(argument) + 1),
                ));
                if !live {
                    assert!(matches!(
                        result,
                        Err(MachineSagaHandoffError::Source(MachineStoreError::Conflict))
                    ));
                } else if !authorized {
                    assert!(matches!(
                        result,
                        Err(MachineSagaHandoffError::Port(PortError::Unauthorized))
                    ));
                } else {
                    let actual = result.unwrap();
                    if let Some(expected) = &receipt {
                        assert_eq!(&actual, expected);
                    } else {
                        assert_eq!(actual.commit().outcomes.len(), 3);
                        assert_eq!(actual.commit().actions.len(), 1);
                        receipt = Some(actual);
                    }
                }
            }
            3 => {
                if let Some(receipt) = &receipt {
                    let result = block_on(outbox.acknowledge(&true, lease, receipt));
                    if !current || (!live && !acknowledged) {
                        assert_eq!(result, Err(MachineStoreError::Conflict));
                    } else if !authorized {
                        assert_eq!(result, Err(MachineStoreError::Denied));
                    } else {
                        assert_eq!(result.unwrap().source, source_identity);
                        acknowledged = true;
                    }
                }
            }
            4 => {
                drop(outbox);
                drop(receiver);
                drop(native);
                drop(source);
                source = make_source();
                native = make_native();
                receiver = make_receiver(native.clone());
                outbox = SqliteMachineOutbox::new(
                    source.clone(),
                    clock.clone(),
                    receiver.clone(),
                    LIMITS,
                )
                .unwrap();
            }
            5 => {
                assert!(matches!(
                    block_on(receiver.accept(&outbox, &false, lease, LogicalTime(1))),
                    Err(MachineSagaHandoffError::Source(MachineStoreError::Denied))
                ));
            }
            6 => {
                authorized = argument & 1 == 0;
                policy.0.store(authorized, Ordering::SeqCst);
            }
            7 => {
                let changed = Arc::new(
                    MachineSagaReceiver::new(
                        native.clone(),
                        policy.clone(),
                        MachineSourceId::from_bytes([99; 32]),
                        id("pri_fuzz"),
                        definition(graph),
                        64,
                    )
                    .unwrap(),
                );
                let changed_outbox = SqliteMachineOutbox::new(
                    source.clone(),
                    clock.clone(),
                    changed.clone(),
                    LIMITS,
                )
                .unwrap();
                assert!(matches!(
                    block_on(changed.accept(&changed_outbox, &true, lease, LogicalTime(1))),
                    Err(MachineSagaHandoffError::Source(MachineStoreError::Conflict))
                ));
                let result = block_on(changed_outbox.claim(&true, claim()));
                if !acknowledged && tick >= expiry {
                    assert_eq!(result.err(), Some(MachineStoreError::Conflict));
                } else {
                    assert!(result.unwrap().is_empty());
                }
            }
            _ => {
                assert_eq!(
                    block_on(outbox.validate(&true, lease)),
                    if live {
                        Ok(())
                    } else {
                        Err(MachineStoreError::Conflict)
                    }
                );
            }
        }
        let status = block_on(outbox.status(&true, SCOPE, source_identity.intent.identity))
            .unwrap()
            .unwrap();
        assert_eq!(status.consumer, Some(receiver.identity()));
        assert_eq!(status.attempt.get(), attempts);
        assert_eq!(status.accepted.is_some(), acknowledged);
        let page = block_on(
            native.read_outcomes(
                &OutcomeReplayRequest::new(
                    native_scope.clone(),
                    0,
                    std::num::NonZeroU16::new(4).unwrap(),
                )
                .unwrap(),
            ),
        )
        .unwrap();
        assert_eq!(page.outcomes.len(), if receipt.is_some() { 3 } else { 0 });
        if let Some(expected) = &receipt {
            assert_eq!(page.outcomes, expected.commit().outcomes);
        }
    }
});
