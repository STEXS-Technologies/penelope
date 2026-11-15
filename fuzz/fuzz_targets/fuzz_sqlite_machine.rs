#![no_main]
//! Bounded real journal operations against an independent arithmetic/history oracle.
use libfuzzer_sys::fuzz_target;
use penelope_core::{machine::*, task::Digest};
use penelope_ports::machine::*;
use penelope_sqlite::{SqliteLimits, SqliteMachineStore};
use std::sync::Arc;
#[path = "../../crates/penelope-sqlite/tests/support/machines.rs"]
#[allow(dead_code)] // Other fixture helpers serve integration/crash profiles.
mod support;
use support::*;

fuzz_target!(|data: &[u8]| {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let limits = SqliteLimits {
        max_history_records: 4,
        max_scope_bytes: 4096,
        ..SqliteLimits::default()
    };
    let open = || SqliteMachineStore::<_, 2>::open(&path, limits, PAYLOADS, Policy, 4).unwrap();
    let mut store = open();
    block_on(store.bootstrap(&true, genesis())).unwrap();
    let mut state = 5_u64;
    // This history contains only independently expected accepted operations.
    let mut commits: Vec<(u8, u64, u64, Arc<MachineCommitBatch<2>>)> = Vec::new();
    for operation in data.chunks(3).take(24) {
        let byte = |index| operation.get(index).copied().unwrap_or(0);
        match byte(0) % 8 {
            0 => {
                let amount = u64::from(byte(1)) + 1;
                let id = byte(2) % 16;
                let head = MachineHead {
                    scope: SCOPE,
                    revision: MachineRevision::new(commits.len() as u64),
                };
                let mut native =
                    MemoryMachine::<_, 2>::restore(Model, head, state, INTENTS).unwrap();
                let request = batch(&mut native, amount, id);
                let result = block_on(store.commit(&true, Arc::clone(&request)));
                if commits.iter().any(|(old, _, _, _)| *old == id) {
                    assert_eq!(result.err(), Some(MachineStoreError::Conflict));
                } else if commits.len() == 4 {
                    assert_eq!(result.err(), Some(MachineStoreError::Capacity));
                } else {
                    let receipt = result.unwrap();
                    state += amount;
                    assert_eq!(receipt.head.revision.get(), commits.len() as u64 + 1);
                    assert_eq!(receipt.disposition, MachineCommitDisposition::Committed);
                    commits.push((id, amount, state, request));
                }
            }
            1 => {
                if let Some((_, _, _, request)) =
                    commits.get(usize::from(byte(1)) % commits.len().max(1))
                {
                    let receipt = block_on(store.commit(&true, Arc::clone(request))).unwrap();
                    assert_eq!(
                        receipt.disposition,
                        MachineCommitDisposition::AlreadyCommitted
                    );
                    assert_eq!(receipt.identity, request.record().identity());
                }
            }
            2 => {
                let snapshot = block_on(store.load_head(&true, SCOPE)).unwrap().unwrap();
                assert_eq!(snapshot.head().revision.get(), commits.len() as u64);
                assert_eq!(snapshot.state(), state.to_le_bytes());
                assert_eq!(snapshot.digest(), Digest::of(&state.to_le_bytes()).0);
            }
            3 => {
                drop(store);
                store = open();
            }
            4 => {
                let id = byte(1) % 16;
                let found =
                    block_on(store.lookup(&true, SCOPE, MachineEventId::from_bytes([id; 32])))
                        .unwrap();
                let expected = commits.iter().find(|(old, _, _, _)| *old == id);
                assert_eq!(found.is_some(), expected.is_some());
                if let (Some(stored), Some((_, amount, result, _))) = (found, expected) {
                    assert_eq!(stored.batch().event(), amount.to_le_bytes());
                    assert_eq!(stored.batch().state(), result.to_le_bytes());
                    assert_eq!(
                        stored.batch().intents().next().unwrap().1,
                        result.to_le_bytes()
                    );
                }
            }
            5 => {
                let count = usize::from(byte(1) % 4) + 1;
                let bytes = usize::from(byte(2) % 5) * 425;
                let result = block_on(store.history(
                    &true,
                    MachineHistoryRequest {
                        scope: SCOPE,
                        after: MachineRevision::ZERO,
                        through: MachineRevision::new(commits.len() as u64),
                        max_records: count as u32,
                        max_bytes: bytes,
                    },
                ));
                if bytes == 0 {
                    assert_eq!(result.err(), Some(MachineStoreError::Capacity));
                } else {
                    let page = result.unwrap();
                    let expected = commits.len().min(count).min(bytes / 425);
                    assert_eq!(page.records.len(), expected);
                    assert_eq!(
                        page.next,
                        if expected < commits.len() {
                            Some(MachineRevision::new(expected as u64))
                        } else {
                            None
                        }
                    );
                    for (stored, (_, _, state, _)) in page.records.iter().zip(&commits) {
                        assert_eq!(stored.batch().state(), state.to_le_bytes());
                    }
                }
            }
            6 => {
                assert_eq!(
                    block_on(store.commit(&false, batch(&mut owner(), 1, 99))).err(),
                    Some(MachineStoreError::Denied)
                );
                assert_eq!(
                    block_on(store.load_head(&false, SCOPE)).err(),
                    Some(MachineStoreError::Denied)
                );
            }
            _ => block_on(store.bootstrap(&true, genesis())).unwrap(),
        }
        let snapshot = block_on(store.load_head(&true, SCOPE)).unwrap().unwrap();
        assert_eq!(snapshot.state(), state.to_le_bytes());
        assert_eq!(snapshot.head().revision.get(), commits.len() as u64);
    }
});
