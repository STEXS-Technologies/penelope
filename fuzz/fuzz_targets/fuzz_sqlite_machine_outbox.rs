#![no_main]
//! Independent single-intent lease/expiry/acceptance oracle over real SQLite.
use async_trait::async_trait;
use libfuzzer_sys::fuzz_target;
use penelope_core::{machine::*, task::Digest};
use penelope_ports::machine::*;
use penelope_sqlite::{SqliteLimits, SqliteMachineOutbox, SqliteMachineStore};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
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
struct Receiver(u8);
#[async_trait]
impl MachineIntentAcceptor for Receiver {
    type Receipt = u8;
    fn identity(&self) -> MachineSourceId {
        MachineSourceId::from_bytes([self.0; 32])
    }
    async fn verify(
        &self,
        source: &MachineIntentSource,
        payload: &[u8],
        receipt: &u8,
    ) -> Result<MachineIntentAcceptance, MachineStoreError> {
        if *receipt != 42 || payload != 13_u64.to_le_bytes() {
            return Err(MachineStoreError::Conflict);
        }
        Ok(MachineIntentAcceptance {
            source: *source,
            consumer: self.identity(),
            receipt_digest: Digest::of(&[*receipt]).0,
        })
    }
}
const LIMITS: MachineOutboxLimits = MachineOutboxLimits {
    lease_ticks: 10,
    max_claims: 1,
    max_claim_bytes: 512,
    max_attempts: 3,
};
fuzz_target!(|data: &[u8]| {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let make = || Arc::new(open(&path));
    let mut store = make();
    block_on(store.bootstrap(&true, genesis())).unwrap();
    let receipt = block_on(store.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let mut outbox = SqliteMachineOutbox::new(
        Arc::clone(&store),
        Arc::clone(&clock),
        Arc::new(Receiver(8)),
        LIMITS,
    )
    .unwrap();
    let mut tick = 100_u64;
    let mut attempts = 0_u64;
    let mut expires = 0_u64;
    let mut accepted = false;
    let mut leases: Vec<MachineIntentLease> = Vec::new();
    for op in data.chunks(2).take(36) {
        let argument = op.get(1).copied().unwrap_or(0);
        match op[0] % 8 {
            0 => {
                let small = argument & 1 != 0;
                let result = block_on(outbox.claim(
                    &true,
                    MachineIntentClaim {
                        scope: SCOPE,
                        owner: MachineLeaseOwner::from_bytes([argument; 32]),
                        max_intents: 1,
                        max_bytes: if small { 311 } else { 312 },
                    },
                ));
                let eligible = !accepted && (attempts == 0 || expires <= tick);
                if eligible && (small || attempts == 3) {
                    assert_eq!(result.err(), Some(MachineStoreError::Capacity));
                } else {
                    let mut result = result.unwrap();
                    assert_eq!(result.len(), usize::from(eligible));
                    if eligible {
                        attempts += 1;
                        expires = tick + 10;
                        let lease = result.remove(0);
                        let metadata = lease.metadata();
                        assert_eq!(metadata.token.get(), attempts);
                        assert_eq!(metadata.attempt.get(), attempts);
                        assert_eq!(metadata.expires.get(), expires);
                        assert_eq!(metadata.source.commit, receipt.identity);
                        assert_eq!(metadata.source.head, receipt.head);
                        assert_eq!(lease.payload(), 13_u64.to_le_bytes());
                        assert_eq!(lease.requires_reconciliation(), attempts > 1);
                        leases.push(lease);
                    }
                }
            }
            1 => {
                tick += u64::from(argument % 16);
                clock.0.store(tick, Ordering::SeqCst);
            }
            2 | 3 => {
                if let Some(lease) = leases.get(usize::from(argument) % leases.len().max(1)) {
                    let current = lease.metadata().token.get() == attempts;
                    let live = current && !accepted && tick < expires;
                    if op[0] % 8 == 2 {
                        assert_eq!(
                            block_on(outbox.validate(&true, lease)),
                            if live {
                                Ok(())
                            } else {
                                Err(MachineStoreError::Conflict)
                            }
                        );
                    } else {
                        let native_receipt = if argument & 1 == 0 { 42 } else { 41 };
                        let result = block_on(outbox.acknowledge(&true, lease, &native_receipt));
                        if current && (live || accepted) && native_receipt == 42 {
                            assert_eq!(result.unwrap().source, lease.metadata().source);
                            accepted = true;
                        } else {
                            assert_eq!(result, Err(MachineStoreError::Conflict));
                        }
                    }
                }
            }
            4 => {
                drop(outbox);
                drop(store);
                store = make();
                outbox = SqliteMachineOutbox::new(
                    Arc::clone(&store),
                    Arc::clone(&clock),
                    Arc::new(Receiver(8)),
                    LIMITS,
                )
                .unwrap();
            }
            5 => {
                assert_eq!(
                    block_on(outbox.status(&false, SCOPE, MachineIntentId::from_bytes([4; 32]))),
                    Err(MachineStoreError::Denied)
                );
            }
            6 => {
                assert_eq!(
                    block_on(outbox.status(&true, SCOPE, MachineIntentId::from_bytes([5; 32])))
                        .unwrap(),
                    None
                );
            }
            _ => {
                if attempts > 0 {
                    let changed = SqliteMachineOutbox::new(
                        Arc::clone(&store),
                        Arc::clone(&clock),
                        Arc::new(Receiver(9)),
                        LIMITS,
                    )
                    .unwrap();
                    for lease in &leases {
                        assert_eq!(
                            block_on(changed.validate(&true, lease)),
                            Err(MachineStoreError::Conflict)
                        );
                        assert_eq!(
                            block_on(changed.acknowledge(&true, lease, &42)),
                            Err(MachineStoreError::Conflict)
                        );
                    }
                    let result = block_on(changed.claim(
                        &true,
                        MachineIntentClaim {
                            scope: SCOPE,
                            owner: MachineLeaseOwner::from_bytes([9; 32]),
                            max_intents: 1,
                            max_bytes: 512,
                        },
                    ));
                    if !accepted && expires <= tick {
                        assert_eq!(result.err(), Some(MachineStoreError::Conflict));
                    } else {
                        assert!(result.unwrap().is_empty());
                    }
                }
            }
        }
        let status = block_on(outbox.status(&true, SCOPE, MachineIntentId::from_bytes([4; 32])))
            .unwrap()
            .unwrap();
        assert_eq!(status.source.commit, receipt.identity);
        assert_eq!(status.attempt.get(), attempts);
        assert_eq!(
            status.consumer,
            if attempts > 0 {
                Some(MachineSourceId::from_bytes([8; 32]))
            } else {
                None
            }
        );
        assert_eq!(status.accepted.is_some(), accepted);
        let head = block_on(store.load_head(&true, SCOPE)).unwrap().unwrap();
        assert_eq!(head.head(), receipt.head);
        assert_eq!(head.state(), 13_u64.to_le_bytes());
    }
});
