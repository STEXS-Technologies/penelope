//! Production machine journal: trusted host policy, atomic bytes and reopen.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic_in_result_fn
)]
use penelope_core::machine::*;
use penelope_ports::machine::*;
use penelope_sqlite::{SqliteLimits, SqliteMachineStore};
use std::sync::Arc;
#[path = "support/machines.rs"]
mod support;
use support::*;

#[test]
fn lost_ack_reopen_exact_duplicate_and_pending_payload_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let store = open(&path);
    block_on(store.bootstrap(&true, genesis())).unwrap();
    let retained = batch(&mut owner(), 8, 4);
    let receipt = block_on(store.commit(&true, Arc::clone(&retained))).unwrap();
    assert_eq!(receipt.disposition, MachineCommitDisposition::Committed);
    drop(store); // Local preparation was dropped; authoritative journal retains the batch.
    let reopened = open(&path);
    let restored = restore(&reopened);
    assert_eq!(*restored.read().1, 13);
    assert_eq!(restored.pending_intents().count(), 0);
    let found = block_on(reopened.lookup(&true, SCOPE, receipt.event))
        .unwrap()
        .unwrap();
    assert_eq!(found.receipt().identity, receipt.identity);
    assert_eq!(
        found.batch().intents().next().unwrap().1,
        13_u64.to_le_bytes()
    );
    let duplicate = block_on(reopened.commit(&true, retained)).unwrap();
    assert_eq!(duplicate, found.receipt());
    block_on(reopened.bootstrap(&true, genesis())).unwrap();
    assert_eq!(*restore(&reopened).read().1, 13);
}

#[test]
fn authorization_precedes_existence_duplicates_and_all_reads() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory.path().join("machine.db"));
    let retained = batch(&mut owner(), 8, 4);
    assert_eq!(
        block_on(store.bootstrap(&false, genesis())).err(),
        Some(MachineStoreError::Denied)
    );
    assert_eq!(
        block_on(store.commit(&false, Arc::clone(&retained))).err(),
        Some(MachineStoreError::Denied)
    );
    assert_eq!(
        block_on(store.load_head(&false, SCOPE)).err(),
        Some(MachineStoreError::Denied)
    );
    assert_eq!(
        block_on(store.lookup(&false, SCOPE, MachineEventId::from_bytes([4; 32]))).err(),
        Some(MachineStoreError::Denied)
    );
    assert_eq!(
        block_on(store.history(&false, history_request())).err(),
        Some(MachineStoreError::Denied)
    );
    assert!(block_on(store.load_head(&true, SCOPE)).unwrap().is_none());
    block_on(store.bootstrap(&true, genesis())).unwrap();
    block_on(store.commit(&true, Arc::clone(&retained))).unwrap();
    assert_eq!(
        block_on(store.commit(&false, retained)).err(),
        Some(MachineStoreError::Denied)
    );
    let wrong = MachineScope {
        definition: MachineDefinition {
            event_schema: 2,
            ..DEFINITION
        },
        ..SCOPE
    };
    assert_eq!(
        block_on(store.load_head(&true, wrong)).err(),
        Some(MachineStoreError::ScopeMismatch)
    );
    let foreign = MachineScope {
        tenant: MachineTenantId::from_bytes([99; 32]),
        ..SCOPE
    };
    assert_eq!(
        block_on(store.load_head(&true, foreign)).err(),
        Some(MachineStoreError::Denied)
    );
}

#[test]
fn competing_heads_conflicting_event_and_wrong_predecessor_leave_exact_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let first = open(&path);
    let second = open(&path);
    block_on(first.bootstrap(&true, genesis())).unwrap();
    let accepted = batch(&mut owner(), 8, 4);
    let stale = batch(&mut owner(), 9, 5);
    let conflict = batch(&mut owner(), 9, 4);
    block_on(first.commit(&true, Arc::clone(&accepted))).unwrap();
    assert_eq!(
        block_on(second.commit(&true, stale)).err(),
        Some(MachineStoreError::StaleHead)
    );
    assert_eq!(
        block_on(second.commit(&true, conflict)).err(),
        Some(MachineStoreError::Conflict)
    );
    let head = restore(&first).read().0;
    let mut forged = MemoryMachine::<_, 2>::restore(Model, head, 99, INTENTS).unwrap();
    let wrong = batch(&mut forged, 1, 7);
    assert_eq!(
        block_on(second.commit(&true, wrong)).err(),
        Some(MachineStoreError::Conflict)
    );
    assert_eq!(*restore(&first).read().1, 13);
    assert_eq!(
        block_on(second.commit(&true, accepted))
            .unwrap()
            .disposition,
        MachineCommitDisposition::AlreadyCommitted
    );
}

#[test]
fn bounded_history_is_stable_across_later_writes_and_never_skips_oversized_records() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory.path().join("machine.db"));
    block_on(store.bootstrap(&true, genesis())).unwrap();
    for id in 1..=3 {
        let retained = batch(&mut restore(&store), 1, id);
        block_on(store.commit(&true, retained)).unwrap();
    }
    let first = block_on(store.history(&true, history_request())).unwrap();
    assert_eq!(first.records.len(), 2);
    assert_eq!(first.next, Some(MachineRevision::new(2)));
    let retained = batch(&mut restore(&store), 1, 4);
    block_on(store.commit(&true, retained)).unwrap();
    let next = block_on(store.history(
        &true,
        MachineHistoryRequest {
            after: first.next.unwrap(),
            ..history_request()
        },
    ))
    .unwrap();
    assert_eq!(next.records.len(), 1);
    assert!(next.next.is_none());
    assert_eq!(
        next.records[0].receipt().head.revision,
        MachineRevision::new(3)
    );
    assert_eq!(
        block_on(store.history(
            &true,
            MachineHistoryRequest {
                max_bytes: 424,
                ..history_request()
            }
        ))
        .err(),
        Some(MachineStoreError::Capacity)
    );
    let single = block_on(store.history(
        &true,
        MachineHistoryRequest {
            max_bytes: 425,
            ..history_request()
        },
    ))
    .unwrap();
    assert_eq!(single.records.len(), 1);
    assert_eq!(single.next, Some(MachineRevision::new(1)));
}

#[test]
fn suppressed_write_rolls_back_entire_batch_and_quota_refusal_is_atomic() {
    for table in ["machine_events", "machine_intents", "machine_heads"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("machine.db");
        let store = open(&path);
        block_on(store.bootstrap(&true, genesis())).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        let operation = if table == "machine_heads" {
            "UPDATE"
        } else {
            "INSERT"
        };
        connection.execute_batch(&format!("CREATE TRIGGER suppress BEFORE {operation} ON {table} BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
        assert_eq!(
            block_on(store.commit(&true, batch(&mut owner(), 8, 4))).err(),
            Some(MachineStoreError::Corrupt)
        );
        assert_eq!(*restore(&store).read().1, 5);
        assert!(
            block_on(store.lookup(&true, SCOPE, MachineEventId::from_bytes([4; 32])))
                .unwrap()
                .is_none()
        );
        let counts:(u32,u32)=connection.query_row("SELECT (SELECT count(*) FROM machine_events),(SELECT count(*) FROM machine_intents)",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(counts, (0, 0));
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("quota.db");
    let invalid_store = SqliteMachineStore::<_, 2>::open(
        &path,
        SqliteLimits {
            max_scope_bytes: 440,
            ..SqliteLimits::default()
        },
        PAYLOADS,
        Policy,
        8,
    );
    // The configured batch maximum must fit the scope before opening.
    assert_eq!(invalid_store.err(), Some(MachineStoreError::Capacity));
    let store = SqliteMachineStore::<_, 2>::open(
        &path,
        SqliteLimits {
            max_scope_bytes: 440,
            ..SqliteLimits::default()
        },
        MachinePayloadLimits {
            max_batch_bytes: 425,
            ..PAYLOADS
        },
        Policy,
        8,
    )
    .unwrap();
    block_on(store.bootstrap(&true, genesis())).unwrap();
    assert_eq!(
        block_on(store.commit(&true, batch(&mut owner(), 8, 4))).err(),
        Some(MachineStoreError::Capacity)
    );
    assert_eq!(*restore(&store).read().1, 5);
}

#[path = "support/write_snapshot.rs"]
mod write_snapshot;
#[test]
fn corrupted_authoritative_coordinates_fail_closed_without_mutating_rows() {
    for damage in [
        "UPDATE machine_heads SET revision=x'0000000000000002'",
        "UPDATE machine_heads SET state=zeroblob(8)",
        "UPDATE machine_heads SET state_digest=zeroblob(32)",
        "UPDATE machine_heads SET last_event=zeroblob(32)",
        "UPDATE machine_heads SET event_count=0",
        "UPDATE machine_events SET event_payload=zeroblob(8)",
        "UPDATE machine_events SET state=zeroblob(8)",
        "UPDATE machine_events SET record=zeroblob(401)",
        "UPDATE machine_events SET revision=x'0000000000000002'",
        "UPDATE machine_intents SET intent=zeroblob(32)",
        "UPDATE machine_intents SET ordinal=1",
        "UPDATE machine_intents SET payload=zeroblob(8)",
        "DELETE FROM machine_intents",
        "DELETE FROM machine_events",
        "DELETE FROM machine_heads",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("machine.db");
        let store = open(&path);
        block_on(store.bootstrap(&true, genesis())).unwrap();
        let retained = batch(&mut owner(), 8, 4);
        block_on(store.commit(&true, Arc::clone(&retained))).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "foreign_keys", "OFF")
            .unwrap(); // Inject otherwise impossible orphan corruption.
        connection.execute_batch(damage).unwrap();
        let tables = ["machine_heads", "machine_events", "machine_intents"];
        let before = write_snapshot::snapshot(&connection, &tables);
        assert_eq!(
            block_on(store.load_head(&true, SCOPE)).err(),
            Some(MachineStoreError::Corrupt),
            "{damage}"
        );
        assert_eq!(
            block_on(store.lookup(&true, SCOPE, MachineEventId::from_bytes([4; 32]))).err(),
            Some(MachineStoreError::Corrupt),
            "{damage}"
        );
        assert_eq!(
            block_on(store.commit(&true, retained)).err(),
            Some(MachineStoreError::Corrupt),
            "{damage}"
        );
        assert_eq!(
            before,
            write_snapshot::snapshot(&connection, &tables),
            "{damage}"
        );
    }
}

#[test]
fn simultaneous_independent_workers_admit_one_competing_head() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let first = open(&path);
    let second = open(&path);
    block_on(first.bootstrap(&true, genesis())).unwrap();
    let one = batch(&mut owner(), 8, 4);
    let two = batch(&mut owner(), 9, 5);
    let barrier = std::sync::Barrier::new(2);
    let (first_result, second_result) = std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            barrier.wait();
            block_on(first.commit(&true, one))
        });
        barrier.wait();
        let result = block_on(second.commit(&true, two));
        (handle.join().unwrap(), result)
    });
    assert_eq!(
        usize::from(first_result.is_ok()) + usize::from(second_result.is_ok()),
        1
    );
    assert!(matches!(
        (&first_result, &second_result),
        (Ok(_), Err(MachineStoreError::StaleHead)) | (Err(MachineStoreError::StaleHead), Ok(_))
    ));
    let restored = restore(&first);
    assert_eq!(restored.read().0.revision, MachineRevision::new(1));
    assert!([13, 14].contains(restored.read().1));
}

#[test]
fn concurrent_reader_sees_one_coherent_state_head_receipt_tuple() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let writer = open(&path);
    let reader = open(&path);
    block_on(writer.bootstrap(&true, genesis())).unwrap();
    std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            for id in 1..=32 {
                let retained = batch(&mut restore(&writer), 1, id);
                block_on(writer.commit(&true, retained)).unwrap();
            }
        });
        for _ in 0..64 {
            let snapshot = block_on(reader.load_head(&true, SCOPE)).unwrap().unwrap();
            let revision = snapshot.head().revision.get();
            assert_eq!(snapshot.state(), (5 + revision).to_le_bytes());
            assert_eq!(
                snapshot.last().map(|receipt| receipt.head.revision.get()),
                if revision == 0 { None } else { Some(revision) }
            );
        }
        handle.join().unwrap();
    });
    assert_eq!(*restore(&reader).read().1, 37);
}

#[test]
fn post_write_substitution_and_deferred_commit_failure_never_issue_success_receipts() {
    for damage in [
        "CREATE TRIGGER damage AFTER INSERT ON machine_events BEGIN UPDATE machine_events SET event_payload=zeroblob(8); END;",
        "CREATE TRIGGER damage AFTER INSERT ON machine_intents BEGIN UPDATE machine_intents SET payload=zeroblob(8); END;",
        "CREATE TRIGGER damage AFTER UPDATE ON machine_heads BEGIN UPDATE machine_heads SET bytes_used=0; END;",
        "CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE blocker(id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER damage AFTER INSERT ON machine_events BEGIN INSERT INTO blocker VALUES(99); END;",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("machine.db");
        let store = open(&path);
        block_on(store.bootstrap(&true, genesis())).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch(damage).unwrap();
        let tables = ["machine_heads", "machine_events", "machine_intents"];
        let before = write_snapshot::snapshot(&connection, &tables);
        let retained = batch(&mut owner(), 8, 4);
        let error = block_on(store.commit(&true, Arc::clone(&retained))).unwrap_err();
        assert!([MachineStoreError::Corrupt, MachineStoreError::Conflict].contains(&error));
        assert_eq!(before, write_snapshot::snapshot(&connection, &tables));
        assert_eq!(*restore(&store).read().1, 5);
        connection.execute_batch("DROP TRIGGER damage").unwrap();
        assert_eq!(
            block_on(store.commit(&true, retained)).unwrap().disposition,
            MachineCommitDisposition::Committed
        );
        assert_eq!(*restore(&store).read().1, 13);
    }
}

#[test]
fn conflicting_genesis_scope_count_and_exact_scope_bytes_keep_existing_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let store = SqliteMachineStore::<_, 2>::open(
        &path,
        SqliteLimits {
            max_scopes: 1,
            max_scope_bytes: 441,
            ..SqliteLimits::default()
        },
        MachinePayloadLimits {
            max_batch_bytes: 425,
            ..PAYLOADS
        },
        Policy,
        4,
    )
    .unwrap();
    block_on(store.bootstrap(&true, genesis())).unwrap();
    let different =
        prepare_machine_bootstrap(&Model, &Codec, SCOPE, &6, &mut [0; 8], PAYLOADS).unwrap();
    assert_eq!(
        block_on(store.bootstrap(&true, Arc::new(different))).err(),
        Some(MachineStoreError::Conflict)
    );
    let second_scope = MachineScope {
        instance: MachineId::from_bytes([99; 32]),
        ..SCOPE
    };
    let second =
        prepare_machine_bootstrap(&Model, &Codec, second_scope, &5, &mut [0; 8], PAYLOADS).unwrap();
    assert_eq!(
        block_on(store.bootstrap(&true, Arc::new(second))).err(),
        Some(MachineStoreError::Capacity)
    );
    block_on(store.commit(&true, batch(&mut owner(), 8, 4))).unwrap(); // Exactly 16 + 425 charged bytes.
    assert_eq!(*restore(&store).read().1, 13);
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT bytes_used FROM machine_heads", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        441
    );
}

#[test]
fn second_intent_identity_conflict_rolls_back_the_entire_inserted_prefix() {
    use penelope_core::task::Digest;
    const MULTI: MachineDefinition = MachineDefinition {
        fingerprint: MachineDefinitionId::from_bytes([44; 32]),
        ..DEFINITION
    };
    const MULTI_SCOPE: MachineScope = MachineScope {
        definition: MULTI,
        ..SCOPE
    };
    struct Two;
    impl Machine for Two {
        type State = u64;
        type Event = u64;
        type Effect = u64;
        type Error = MachineError;
        fn definition(&self) -> MachineDefinition {
            MULTI
        }
        fn validate(&self, _: &u64) -> Result<(), MachineError> {
            Ok(())
        }
        fn transition<S: IntentSink<u64>>(
            &self,
            state: &u64,
            event: &u64,
            context: &TransitionContext,
            sink: &mut S,
        ) -> Result<Transition<u64>, MachineError> {
            let next = state.checked_add(*event).ok_or(MachineError::Capacity)?;
            for identity in [
                MachineIntentId::from_bytes(context.event.bytes()),
                MachineIntentId::from_bytes([99; 32]),
            ] {
                sink.stage(ProposedIntent {
                    metadata: IntentMetadata {
                        identity,
                        digest: Digest::of(&next.to_le_bytes()).0,
                        payload_bytes: 8,
                    },
                    payload: next,
                })?;
            }
            Ok(Transition::Accepted {
                next,
                transition: TransitionId::new(1),
            })
        }
    }
    struct MultiCodec;
    impl MachineCodec<Two> for MultiCodec {
        type Error = ();
        fn definition(&self) -> MachineDefinition {
            MULTI
        }
        fn event_type(&self, _: &u64) -> MachineEventType {
            MachineEventType::new(1)
        }
        fn encode_state(&self, state: &u64, out: &mut [u8]) -> Result<usize, ()> {
            encode(*state, out)
        }
        fn decode_state(&self, bytes: &[u8]) -> Result<u64, ()> {
            decode(bytes)
        }
        fn encode_event(&self, event: &u64, out: &mut [u8]) -> Result<usize, ()> {
            encode(*event, out)
        }
        fn decode_event(&self, kind: MachineEventType, bytes: &[u8]) -> Result<u64, ()> {
            if kind != MachineEventType::new(1) {
                return Err(());
            }
            decode(bytes)
        }
        fn encode_effect(&self, effect: &u64, out: &mut [u8]) -> Result<usize, ()> {
            encode(*effect, out)
        }
        fn decode_effect(&self, bytes: &[u8]) -> Result<u64, ()> {
            decode(bytes)
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("multi.db");
    let store = open(&path);
    let bootstrap =
        prepare_machine_bootstrap(&Two, &MultiCodec, MULTI_SCOPE, &5, &mut [0; 8], PAYLOADS)
            .unwrap();
    block_on(store.bootstrap(&true, Arc::new(bootstrap))).unwrap();
    for id in [4_u8, 5] {
        let snapshot = block_on(store.load_head(&true, MULTI_SCOPE))
            .unwrap()
            .unwrap();
        let mut machine: MemoryMachine<_, 2> = snapshot
            .restore(Two, &MultiCodec, MULTI_SCOPE, &mut [0; 8], INTENTS)
            .unwrap();
        let event = 8_u64;
        let context = TransitionContext {
            expected: machine.read().0,
            event: MachineEventId::from_bytes([id; 32]),
            event_digest: Digest::of(&event.to_le_bytes()).0,
            logical_time: MachineLogicalTime::new(7),
        };
        let mut state = [0; 8];
        let mut input = [0; 8];
        let mut effect = [0; 8];
        let MachineCommitDecision::Prepared(prepared) = prepare_machine_commit(
            &mut machine,
            &MachineCommitInput {
                event: &event,
                context,
                codec: &MultiCodec,
                causation: None,
            },
            &mut MachineCommitBuffers {
                state: &mut state,
                event: &mut input,
                intent: &mut effect,
            },
            PAYLOADS,
        )
        .unwrap() else {
            panic!("accepted multi fixture")
        };
        let result = block_on(store.commit(&true, prepared.batch()));
        if id == 4 {
            result.unwrap();
        } else {
            assert_eq!(result.err(), Some(MachineStoreError::Conflict));
        }
    }
    let snapshot = block_on(store.load_head(&true, MULTI_SCOPE))
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.state(), 13_u64.to_le_bytes());
    assert_eq!(snapshot.head().revision, MachineRevision::new(1));
    let connection = rusqlite::Connection::open(&path).unwrap();
    let counts: (u32, u32) = connection
        .query_row(
            "SELECT (SELECT count(*) FROM machine_events),(SELECT count(*) FROM machine_intents)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 2));
}

#[test]
fn resolved_commits_do_not_wait_for_an_unrelated_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let store = open(&path);
    block_on(store.bootstrap(&true, genesis())).unwrap();
    let accepted = batch(&mut owner(), 8, 4);
    block_on(store.commit(&true, Arc::clone(&accepted))).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        block_on(store.commit(&true, batch(&mut owner(), 9, 5))).err(),
        Some(MachineStoreError::StaleHead)
    );
    assert_eq!(
        block_on(store.commit(&true, batch(&mut owner(), 9, 4))).err(),
        Some(MachineStoreError::Conflict)
    );
    assert_eq!(
        block_on(store.commit(&true, accepted)).unwrap().disposition,
        MachineCommitDisposition::AlreadyCommitted
    );
    connection.execute_batch("ROLLBACK").unwrap();
    assert_eq!(*restore(&store).read().1, 13);
}

#[test]
fn full_history_refuses_new_events_but_preserves_exact_retries_and_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bounded.db");
    let store = SqliteMachineStore::<_, 2>::open(
        &path,
        SqliteLimits {
            max_history_records: 4,
            ..SqliteLimits::default()
        },
        PAYLOADS,
        Policy,
        8,
    )
    .unwrap();
    block_on(store.bootstrap(&true, genesis())).unwrap();
    let retained = batch(&mut owner(), 1, 1);
    let original = block_on(store.commit(&true, Arc::clone(&retained))).unwrap();
    for id in 2..=4 {
        block_on(store.commit(&true, batch(&mut restore(&store), 1, id))).unwrap();
    }
    let connection = rusqlite::Connection::open(&path).unwrap();
    let tables = ["machine_heads", "machine_events", "machine_intents"];
    let before = write_snapshot::snapshot(&connection, &tables);
    assert_eq!(
        block_on(store.commit(&true, batch(&mut restore(&store), 1, 5))).err(),
        Some(MachineStoreError::Capacity)
    );
    assert_eq!(before, write_snapshot::snapshot(&connection, &tables));
    let duplicate = block_on(store.commit(&true, retained)).unwrap();
    assert_eq!(
        duplicate.disposition,
        MachineCommitDisposition::AlreadyCommitted
    );
    assert_eq!(duplicate.identity, original.identity);
    assert_eq!(before, write_snapshot::snapshot(&connection, &tables));
    let restored = restore(&store);
    assert_eq!(restored.read().0.revision, MachineRevision::new(4));
    assert_eq!(*restored.read().1, 9);
}

#[test]
fn every_small_history_partition_recovers_exact_records_across_reopens_and_refuses_a_gap() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine.db");
    let mut store = open(&path);
    block_on(store.bootstrap(&true, genesis())).unwrap();
    let mut expected_state = 5_u64;
    let mut expected = Vec::new();
    for revision in 1..=9_u8 {
        let event = u64::from(revision) * 3;
        expected_state += event;
        let request = batch(&mut restore(&store), event, revision);
        assert_eq!(request.byte_len(), 425);
        let mut receipt = block_on(store.commit(&true, request)).unwrap();
        assert_eq!(receipt.head.revision.get(), u64::from(revision));
        receipt.disposition = MachineCommitDisposition::AlreadyCommitted;
        expected.push((
            receipt,
            event.to_le_bytes().to_vec(),
            expected_state.to_le_bytes().to_vec(),
            vec![(
                IntentMetadata {
                    identity: MachineIntentId::from_bytes([revision; 32]),
                    digest: penelope_core::task::Digest::of(&expected_state.to_le_bytes()).0,
                    payload_bytes: 8,
                },
                expected_state.to_le_bytes().to_vec(),
            )],
        ));
    }
    let signature = |stored: &MachineStoredCommit<2>| {
        (
            stored.receipt(),
            stored.batch().event().to_vec(),
            stored.batch().state().to_vec(),
            stored
                .batch()
                .intents()
                .map(|(metadata, bytes)| (*metadata, bytes.to_vec()))
                .collect::<Vec<_>>(),
        )
    };
    let request = MachineHistoryRequest {
        scope: SCOPE,
        after: MachineRevision::ZERO,
        through: MachineRevision::new(9),
        max_records: 9,
        max_bytes: 9 * 425,
    };
    let contiguous = block_on(store.history(&true, request)).unwrap();
    assert!(contiguous.next.is_none());
    assert_eq!(
        contiguous
            .records
            .iter()
            .map(&signature)
            .collect::<Vec<_>>(),
        expected
    );
    // Later writes must not change any partition of the captured upper revision.
    block_on(store.commit(&true, batch(&mut restore(&store), 99, 10))).unwrap();
    for byte_limited in [false, true] {
        // Eight optional cuts enumerate every partition of nine accepted records.
        for cuts in 0..256_u16 {
            let mut after = 0_u64;
            let mut reconstructed = Vec::new();
            for end in 1..=9_u64 {
                if end != 9 && cuts & (1 << (end - 1)) == 0 {
                    continue;
                }
                drop(store);
                store = open(&path);
                let count = u32::try_from(end - after).unwrap();
                let page = block_on(store.history(
                    &true,
                    MachineHistoryRequest {
                        after: MachineRevision::new(after),
                        max_records: if byte_limited { 9 } else { count },
                        max_bytes: if byte_limited {
                            usize::try_from(count).unwrap() * 425
                        } else {
                            9 * 425
                        },
                        ..request
                    },
                ))
                .unwrap();
                assert_eq!(page.records.len(), usize::try_from(count).unwrap());
                assert_eq!(
                    page.next,
                    if end == 9 {
                        None
                    } else {
                        Some(MachineRevision::new(end))
                    }
                );
                for stored in &page.records {
                    let restored = stored
                        .restore(Model, &Codec, SCOPE, &mut [0; 8], INTENTS)
                        .unwrap();
                    assert_eq!(restored.read().0, stored.receipt().head);
                    assert_eq!(restored.pending_intents().count(), 0);
                }
                reconstructed.extend(page.records.iter().map(&signature));
                after = end;
            }
            assert_eq!(
                reconstructed, expected,
                "cuts={cuts} byte_limited={byte_limited}"
            );
        }
    }
    // A missing record cannot be skipped within or exactly at a page boundary.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    connection
        .execute(
            "DELETE FROM machine_events WHERE revision=?1",
            [5_u64.to_be_bytes().as_slice()],
        )
        .unwrap();
    let first = block_on(store.history(
        &true,
        MachineHistoryRequest {
            max_records: 3,
            ..request
        },
    ))
    .unwrap();
    assert_eq!(first.next, Some(MachineRevision::new(3)));
    assert_eq!(
        block_on(store.history(
            &true,
            MachineHistoryRequest {
                after: first.next.unwrap(),
                ..request
            }
        ))
        .err(),
        Some(MachineStoreError::Corrupt)
    );
    assert_eq!(
        block_on(store.history(
            &true,
            MachineHistoryRequest {
                after: MachineRevision::new(4),
                ..request
            }
        ))
        .err(),
        Some(MachineStoreError::Corrupt)
    );
    assert_eq!(
        block_on(store.history(&true, request)).err(),
        Some(MachineStoreError::Corrupt)
    );
}
