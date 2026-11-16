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
                // An out-of-order future revision cannot become the next event.
                // ID99 is outside the admitted input-ID range0..16.
                if byte(1) & 128 != 0 {
                    let future = MachineHead {
                        revision: MachineRevision::new(head.revision.get() + 1),
                        ..head
                    };
                    let mut premature =
                        MemoryMachine::<_, 2>::restore(Model, future, state, INTENTS).unwrap();
                    assert_eq!(
                        block_on(store.commit(&true, batch(&mut premature, amount, 99))).err(),
                        Some(MachineStoreError::StaleHead)
                    );
                }
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
                // Replay an arbitrary suffix in reverse order. Every accepted
                // request retains its original receipt; none advances the head.
                for (_, _, _, request) in commits.iter().rev().take(usize::from(byte(1) % 4) + 1) {
                    let receipt = block_on(store.commit(&true, Arc::clone(request))).unwrap();
                    assert_eq!(
                        receipt.disposition,
                        MachineCommitDisposition::AlreadyCommitted
                    );
                    assert_eq!(receipt.identity, request.record().identity());
                    assert_eq!(
                        receipt.head.revision,
                        request.record().header().transition.revision
                    );
                    assert_eq!(
                        receipt.event,
                        request.record().header().transition.context.event
                    );
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
                    // Recompute valid framing after changing one identity role.
                    // Structural decoding is not source authentication; pinned
                    // restoration must still refuse every foreign scope/model.
                    let mut header = stored.batch().record().header();
                    let scope = &mut header.transition.context.expected.scope;
                    let variant = byte(2) % 6;
                    match variant {
                        0 => scope.tenant = MachineTenantId::from_bytes([99; 32]),
                        1 => scope.instance = MachineId::from_bytes([99; 32]),
                        2 => {
                            scope.definition.fingerprint = MachineDefinitionId::from_bytes([99; 32])
                        }
                        3 => scope.definition.state_schema = 99,
                        4 => scope.definition.event_schema = 99,
                        _ => header.transition.context.event_digest[0] ^= 1,
                    }
                    let altered = MachineCommitRecord::<2>::new(
                        header,
                        stored.batch().record().intents().copied(),
                    )
                    .unwrap();
                    let mut wire = vec![0; altered.encoded_len()];
                    altered.encode(&mut wire).unwrap();
                    let recovered = MachineStoredCommit::<2>::decode(
                        &wire,
                        stored.batch().event(),
                        stored.batch().state(),
                        stored.batch().intents().map(|(_, payload)| payload),
                        PAYLOADS,
                    );
                    if variant == 5 {
                        assert_eq!(recovered.err(), Some(MachineRecoveryError::PayloadMismatch));
                    } else {
                        let recovered = recovered.unwrap();
                        assert!(
                            recovered
                                .restore(Model, &Codec, SCOPE, &mut [0; 8], INTENTS)
                                .is_err()
                        );
                    }
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
                    let mut replayed = owner();
                    for (stored, (_, _, state, _)) in page.records.iter().zip(&commits) {
                        assert_eq!(stored.batch().state(), state.to_le_bytes());
                        replayed = stored
                            .replay(
                                replayed,
                                &Codec,
                                &mut MachineCommitBuffers {
                                    state: &mut [0; 8],
                                    event: &mut [0; 8],
                                    intent: &mut [0; 8],
                                },
                                PAYLOADS,
                            )
                            .unwrap();
                        assert_eq!(replayed.read(), (stored.receipt().head, state));
                        assert_eq!(replayed.pending_intents().count(), 0);
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
