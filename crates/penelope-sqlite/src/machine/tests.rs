//! Actual process death while the production commit transaction is in flight.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use crate::SqliteMachineStore;
use crate::machine_test_support as support;
use penelope_core::machine::*;
use penelope_ports::machine::*;
use std::{
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};
use support::*;

fn wait_for_marker(child: &mut Child, marker: &Path, phase: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::fs::read(marker).ok().as_deref() != Some(phase.as_bytes()) {
        if Instant::now() >= deadline {
            drop(child.kill());
            drop(child.wait());
            panic!("child failed to reach {phase}");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn cut(phase: &str) {
    if std::env::var("PENELOPE_MACHINE_CUT").ok().as_deref() != Some(phase) {
        return;
    }
    std::fs::write(std::env::var_os("PENELOPE_MACHINE_MARKER").unwrap(), phase).unwrap();
    let resume = std::env::var_os("PENELOPE_MACHINE_RESUME");
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if resume.as_ref().is_some_and(|path| Path::new(path).exists()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("parent did not kill or resume commit child");
}
#[test]
fn commit_process_helper() {
    let Some(path) = std::env::var_os("PENELOPE_MACHINE_DB") else {
        return;
    };
    let store = support::open(Path::new(&path));
    let request = std::env::var("PENELOPE_MACHINE_CHAIN_EVENT").map_or_else(
        |_| batch(&mut owner(), 8, 4),
        |event| {
            let (amount, id) = event.split_once(',').unwrap();
            batch(
                &mut restore(&store),
                amount.parse().unwrap(),
                id.parse().unwrap(),
            )
        },
    );
    block_on(store.commit(&true, request)).unwrap();
    panic!("commit cut not reached");
}
#[test]
fn sigkill_at_each_commit_boundary_recovers_all_or_nothing_and_resolves_lost_ack() {
    for phase in [
        "before-transaction",
        "after-preflight",
        "after-begin",
        "after-event",
        "after-intents",
        "after-head",
        "after-validation",
        "after-commit",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("machines.db");
        let store = support::open(&path);
        block_on(store.bootstrap(&true, genesis())).unwrap();
        drop(store);
        let marker = directory.path().join("marker");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "machine::tests::commit_process_helper",
                "--nocapture",
            ])
            .env("PENELOPE_MACHINE_DB", &path)
            .env("PENELOPE_MACHINE_CUT", phase)
            .env("PENELOPE_MACHINE_MARKER", &marker)
            .spawn()
            .unwrap();
        wait_for_marker(&mut child, &marker, phase);
        child.kill().unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        let reopened = support::open(&path);
        let committed = phase == "after-commit";
        assert_eq!(
            *restore(&reopened).read().1,
            if committed { 13 } else { 5 },
            "{phase}"
        );
        let lookup =
            block_on(reopened.lookup(&true, SCOPE, MachineEventId::from_bytes([4; 32]))).unwrap();
        assert_eq!(lookup.is_some(), committed, "{phase}");
        let connection = rusqlite::Connection::open(&path).unwrap();
        let counts:(u32,u32)=connection.query_row("SELECT (SELECT count(*) FROM machine_events),(SELECT count(*) FROM machine_intents)",[],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(counts, if committed { (1, 1) } else { (0, 0) }, "{phase}");
        let receipt = block_on(reopened.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
        assert_eq!(
            receipt.disposition,
            if committed {
                MachineCommitDisposition::AlreadyCommitted
            } else {
                MachineCommitDisposition::Committed
            }
        );
        assert_eq!(*restore(&reopened).read().1, 13);
        println!("machine phase={phase} signal=9 coherent_reopen=true exact_retry=true");
    }
}

#[test]
fn preflight_race_process_helper() {
    let Some(path) = std::env::var_os("PENELOPE_MACHINE_RACE_DB") else {
        return;
    };
    let store = support::open(Path::new(&path));
    let result = block_on(store.commit(&true, batch(&mut owner(), 8, 4)));
    match std::env::var("PENELOPE_MACHINE_RACE").unwrap().as_str() {
        "duplicate" => {
            let receipt = result.unwrap();
            assert_eq!(
                receipt.disposition,
                MachineCommitDisposition::AlreadyCommitted
            );
            let stored = block_on(store.lookup(&true, SCOPE, receipt.event))
                .unwrap()
                .unwrap();
            assert_eq!(receipt, stored.receipt());
        }
        "conflict" => assert_eq!(result.err(), Some(MachineStoreError::Conflict)),
        "stale" => assert_eq!(result.err(), Some(MachineStoreError::StaleHead)),
        _ => panic!("unknown preflight race case"),
    }
}

#[test]
fn commit_revalidates_duplicate_conflict_and_stale_after_preflight_snapshot() {
    for (case, amount, event) in [("duplicate", 8, 4), ("conflict", 9, 4), ("stale", 9, 5)] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("race.db");
        let store = support::open(&path);
        block_on(store.bootstrap(&true, genesis())).unwrap();
        let marker = directory.path().join("marker");
        let resume = directory.path().join("resume");
        let phase = "after-preflight";
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "machine::tests::preflight_race_process_helper",
                "--nocapture",
            ])
            .env("PENELOPE_MACHINE_RACE_DB", &path)
            .env("PENELOPE_MACHINE_RACE", case)
            .env("PENELOPE_MACHINE_CUT", phase)
            .env("PENELOPE_MACHINE_MARKER", &marker)
            .env("PENELOPE_MACHINE_RESUME", &resume)
            .spawn()
            .unwrap();
        wait_for_marker(&mut child, &marker, phase);
        let accepted = batch(&mut owner(), amount, event);
        let receipt = block_on(store.commit(&true, std::sync::Arc::clone(&accepted))).unwrap();
        assert_eq!(receipt.disposition, MachineCommitDisposition::Committed);
        let connection = rusqlite::Connection::open(&path).unwrap();
        let tables = ["machine_heads", "machine_events", "machine_intents"];
        let before = crate::write_snapshot::snapshot(&connection, &tables);
        std::fs::write(&resume, []).unwrap();
        assert!(child.wait().unwrap().success(), "{case}");
        assert_eq!(
            before,
            crate::write_snapshot::snapshot(&connection, &tables),
            "{case}"
        );
        let restored = restore(&store);
        assert_eq!(restored.read().0.revision, MachineRevision::new(1));
        assert_eq!(*restored.read().1, 5 + amount);
        let retained = block_on(store.lookup(&true, SCOPE, receipt.event))
            .unwrap()
            .unwrap();
        assert_eq!(
            retained.batch().record().identity(),
            accepted.record().identity()
        );
        assert!(retained.batch().intents().eq(accepted.intents()));
        println!(
            "preflight case={case} intervening_commit=true child_revalidated=true unchanged_rows=true"
        );
    }
}

struct LeaseClock;
impl MachineLeaseClock for LeaseClock {
    fn now(&self) -> Result<MachineLeaseTick, MachineStoreError> {
        Ok(MachineLeaseTick::new(100))
    }
}
struct AcceptedReceiver;
#[async_trait::async_trait]
impl MachineIntentAcceptor for AcceptedReceiver {
    type Receipt = ();
    fn identity(&self) -> MachineSourceId {
        MachineSourceId::from_bytes([8; 32])
    }
    async fn verify(
        &self,
        source: &MachineIntentSource,
        payload: &[u8],
        (): &(),
    ) -> Result<MachineIntentAcceptance, MachineStoreError> {
        if payload != 13_u64.to_le_bytes() {
            return Err(MachineStoreError::Conflict);
        }
        Ok(MachineIntentAcceptance {
            source: *source,
            consumer: self.identity(),
            receipt_digest: [6; 32],
        })
    }
}
fn outbox(
    store: std::sync::Arc<SqliteMachineStore<Policy, 2>>,
) -> super::SqliteMachineOutbox<Policy, LeaseClock, AcceptedReceiver, 2> {
    super::SqliteMachineOutbox::new(
        store,
        std::sync::Arc::new(LeaseClock),
        std::sync::Arc::new(AcceptedReceiver),
        MachineOutboxLimits {
            lease_ticks: 10,
            max_claims: 1,
            max_claim_bytes: 512,
            max_attempts: 3,
        },
    )
    .unwrap()
}
const fn claim_request() -> MachineIntentClaim {
    MachineIntentClaim {
        scope: SCOPE,
        owner: MachineLeaseOwner::from_bytes([9; 32]),
        max_intents: 1,
        max_bytes: 512,
    }
}
#[test]
fn outbox_process_helper() {
    let Some(path) = std::env::var_os("PENELOPE_OUTBOX_DB") else {
        return;
    };
    let outbox = outbox(std::sync::Arc::new(support::open(Path::new(&path))));
    if std::env::var("PENELOPE_OUTBOX_ACK").is_ok() {
        let source = block_on(outbox.status(&true, SCOPE, MachineIntentId::from_bytes([4; 32])))
            .unwrap()
            .unwrap()
            .source;
        let lease = MachineIntentLease::decode(
            MachineIntentLeaseMetadata {
                source,
                owner: claim_request().owner,
                token: MachineLeaseToken::new(1),
                expires: MachineLeaseTick::new(110),
                attempt: MachineDeliveryAttempt::new(1),
            },
            &13_u64.to_le_bytes(),
            8,
        )
        .unwrap();
        block_on(outbox.acknowledge(&true, &lease, &())).unwrap();
    } else {
        block_on(outbox.claim(&true, claim_request())).unwrap();
    }
    panic!("outbox cut not reached");
}
#[test]
fn sigkill_outbox_claim_and_ack_preserve_fencing_and_resolve_lost_receipts() {
    for phase in [
        "outbox-claim-before-transaction",
        "outbox-claim-after-token",
        "outbox-claim-after-delivery",
        "outbox-claim-after-usage",
        "outbox-claim-after-validation",
        "outbox-claim-after-commit",
        "outbox-ack-before-transaction",
        "outbox-ack-after-begin",
        "outbox-ack-after-write",
        "outbox-ack-after-validation",
        "outbox-ack-after-commit",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outbox.db");
        let store = std::sync::Arc::new(support::open(&path));
        block_on(store.bootstrap(&true, genesis())).unwrap();
        block_on(store.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
        let adapter = outbox(std::sync::Arc::clone(&store));
        let ack = phase.starts_with("outbox-ack");
        let lease = ack.then(|| {
            block_on(adapter.claim(&true, claim_request()))
                .unwrap()
                .remove(0)
        });
        drop(adapter);
        drop(store);
        let marker = directory.path().join("marker");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "machine::tests::outbox_process_helper",
                "--nocapture",
            ])
            .env("PENELOPE_OUTBOX_DB", &path)
            .env("PENELOPE_MACHINE_CUT", phase)
            .env("PENELOPE_MACHINE_MARKER", &marker);
        if ack {
            command.env("PENELOPE_OUTBOX_ACK", "1");
        }
        let mut child = command.spawn().unwrap();
        wait_for_marker(&mut child, &marker, phase);
        child.kill().unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        let reopened = outbox(std::sync::Arc::new(support::open(&path)));
        let status = block_on(reopened.status(&true, SCOPE, MachineIntentId::from_bytes([4; 32])))
            .unwrap()
            .unwrap();
        let committed = phase.ends_with("after-commit");
        assert_eq!(status.attempt.get(), u64::from(ack || committed), "{phase}");
        assert_eq!(status.accepted.is_some(), ack && committed, "{phase}");
        let connection = rusqlite::Connection::open(&path).unwrap();
        let next: i64 = connection
            .query_row("SELECT next_token FROM lease_counter", [], |row| row.get(0))
            .unwrap();
        assert_eq!(next, if ack || committed { 2 } else { 1 }, "{phase}");
        if let Some(lease) = lease {
            let accepted = block_on(reopened.acknowledge(&true, &lease, &())).unwrap();
            assert_eq!(
                block_on(reopened.status(&true, SCOPE, status.source.intent.identity))
                    .unwrap()
                    .unwrap()
                    .accepted,
                Some(accepted)
            );
        } else if committed {
            assert!(
                block_on(reopened.claim(&true, claim_request()))
                    .unwrap()
                    .is_empty()
            );
        } else {
            let fresh = block_on(reopened.claim(&true, claim_request()))
                .unwrap()
                .remove(0);
            assert_eq!(fresh.metadata().token, MachineLeaseToken::new(1));
        }
    }
}

#[test]
fn sigkill_v7_outbox_migration_preserves_populated_machine_journal() {
    for phase in [
        "before-transaction",
        "after-begin",
        "after-v8",
        "after-v9",
        "before-commit",
        "after-commit",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("v7.db");
        let store = support::open(&path);
        block_on(store.bootstrap(&true, genesis())).unwrap();
        let receipt = block_on(store.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
        drop(store);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("DROP TABLE machine_intent_delivery; ALTER TABLE machine_heads DROP COLUMN lease_tick; ALTER TABLE machine_heads DROP COLUMN delivery_bytes; PRAGMA user_version=7;").unwrap();
        fn rows(connection: &rusqlite::Connection) -> Vec<Vec<u8>> {
            [
                "SELECT scope FROM machine_heads",
                "SELECT state FROM machine_heads",
                "SELECT record FROM machine_events",
                "SELECT payload FROM machine_intents",
            ]
            .into_iter()
            .map(|sql| connection.query_row(sql, [], |row| row.get(0)).unwrap())
            .collect()
        }
        let before = rows(&connection);
        drop(connection);
        let marker = directory.path().join("marker");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "machine::tests::outbox_process_helper",
                "--nocapture",
            ])
            .env("PENELOPE_OUTBOX_DB", &path)
            .env("PENELOPE_MIGRATION_CUT", phase)
            .env("PENELOPE_MIGRATION_MARKER", &marker)
            .spawn()
            .unwrap();
        wait_for_marker(&mut child, &marker, phase);
        child.kill().unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        let recovered_connection = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(rows(&recovered_connection), before, "{phase}");
        let version: u32 = recovered_connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            version,
            if phase == "after-commit" { 9 } else { 7 },
            "{phase}"
        );
        drop(recovered_connection);
        let reopened = std::sync::Arc::new(support::open(&path));
        assert_eq!(*restore(&reopened).read().1, 13);
        let found = block_on(reopened.lookup(&true, SCOPE, receipt.event))
            .unwrap()
            .unwrap();
        assert_eq!(found.receipt().identity, receipt.identity);
        let outbox = outbox(reopened);
        let lease = block_on(outbox.claim(&true, claim_request()))
            .unwrap()
            .remove(0);
        assert_eq!(lease.metadata().attempt.get(), 1);
        assert_eq!(lease.metadata().source.commit, receipt.identity);
    }
}

#[path = "../../tests/support/machine_schema.rs"]
mod machine_schema;
#[test]
fn sigkill_v8_receiver_migration_preserves_pending_uncertainty_and_accepted_receipts() {
    for accepted in [false, true] {
        for phase in [
            "before-transaction",
            "after-begin",
            "after-v9",
            "before-commit",
            "after-commit",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("v8.db");
            let store = std::sync::Arc::new(support::open(&path));
            block_on(store.bootstrap(&true, genesis())).unwrap();
            block_on(store.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
            let original = outbox(std::sync::Arc::clone(&store));
            let lease = block_on(original.claim(&true, claim_request()))
                .unwrap()
                .remove(0);
            if accepted {
                block_on(original.acknowledge(&true, &lease, &())).unwrap();
            }
            drop(original);
            drop(store);
            let connection = rusqlite::Connection::open(&path).unwrap();
            machine_schema::downgrade_to_v8(&connection);
            let tables = [
                "machine_heads",
                "machine_events",
                "machine_intents",
                "machine_intent_delivery",
                "lease_counter",
            ];
            let before = crate::write_snapshot::snapshot(&connection, &tables);
            drop(connection);
            let marker = directory.path().join("marker");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "machine::tests::outbox_process_helper",
                    "--nocapture",
                ])
                .env("PENELOPE_OUTBOX_DB", &path)
                .env("PENELOPE_MIGRATION_CUT", phase)
                .env("PENELOPE_MIGRATION_MARKER", &marker)
                .spawn()
                .unwrap();
            wait_for_marker(&mut child, &marker, phase);
            child.kill().unwrap();
            assert_eq!(child.wait().unwrap().signal(), Some(9));
            let recovered_connection = rusqlite::Connection::open(&path).unwrap();
            assert_eq!(
                crate::write_snapshot::snapshot(&recovered_connection, &tables),
                before
            );
            let version: u32 = recovered_connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, if phase == "after-commit" { 9 } else { 8 });
            drop(recovered_connection);
            let recovered = outbox(std::sync::Arc::new(support::open(&path)));
            let status =
                block_on(recovered.status(&true, SCOPE, lease.metadata().source.intent.identity))
                    .unwrap()
                    .unwrap();
            assert_eq!(status.accepted.is_some(), accepted);
            if accepted {
                block_on(recovered.acknowledge(&true, &lease, &())).unwrap();
            } else {
                assert_eq!(status.consumer, None);
                assert_eq!(
                    block_on(recovered.validate(&true, &lease)),
                    Err(MachineStoreError::Conflict)
                );
            }
        }
    }
}

// Check the entire journal, not just its latest snapshot, after successive deaths.
fn assert_killed_chain(
    path: &Path,
    store: &SqliteMachineStore<Policy, 2>,
    expected: &[std::sync::Arc<MachineCommitBatch<2>>],
    state: u64,
) {
    let snapshot = block_on(store.load_head(&true, SCOPE)).unwrap().unwrap();
    assert_eq!(snapshot.head().revision.get(), expected.len() as u64);
    assert_eq!(snapshot.state(), state.to_le_bytes());
    let page = block_on(store.history(
        &true,
        MachineHistoryRequest {
            scope: SCOPE,
            after: MachineRevision::ZERO,
            through: MachineRevision::new(expected.len() as u64),
            max_records: 32,
            max_bytes: 32 * 1024,
        },
    ))
    .unwrap();
    assert_eq!(page.records.len(), expected.len());
    assert_eq!(page.next, None);
    for (stored, original) in page.records.iter().zip(expected) {
        assert_eq!(stored.batch().record(), original.record());
        assert_eq!(stored.batch().event(), original.event());
        assert_eq!(stored.batch().state(), original.state());
        assert_eq!(
            stored.batch().intents().collect::<Vec<_>>(),
            original.intents().collect::<Vec<_>>()
        );
        let found = block_on(store.lookup(&true, SCOPE, stored.receipt().event))
            .unwrap()
            .unwrap();
        assert_eq!(found.receipt(), stored.receipt());
    }
    let connection = rusqlite::Connection::open(path).unwrap();
    let rows: (u32, u32) = connection
        .query_row(
            "SELECT (SELECT count(*) FROM machine_events), (SELECT count(*) FROM machine_intents)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let count = u32::try_from(expected.len()).unwrap();
    assert_eq!(rows, (count, count));
}

#[test]
fn repeated_sigkill_on_a_growing_journal_preserves_every_prefix_and_exact_retry() {
    let phases = [
        "before-transaction",
        "after-preflight",
        "after-begin",
        "after-event",
        "after-intents",
        "after-head",
        "after-validation",
        "after-commit",
    ];
    for seed in [1_u64, 2, 3] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("chain.db");
        let store = support::open(&path);
        block_on(store.bootstrap(&true, genesis())).unwrap();
        drop(store);
        let mut random = seed;
        let mut state = 5_u64;
        let mut expected = Vec::new();
        for id in 1_u8..=16 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let amount = 1 + (random & 255);
            let phase = *phases
                .get((usize::from(id) - 1 + seed as usize) % phases.len())
                .unwrap();
            // Expected state/revision come from arithmetic and the accepted prefix,
            // never from the production journal's restored snapshot.
            let mut native = MemoryMachine::<_, 2>::restore(
                Model,
                MachineHead {
                    scope: SCOPE,
                    revision: MachineRevision::new(expected.len() as u64),
                },
                state,
                INTENTS,
            )
            .unwrap();
            let request = batch(&mut native, amount, id);
            assert_eq!(request.event(), amount.to_le_bytes());
            assert_eq!(request.state(), (state + amount).to_le_bytes());
            {
                let mut intents = request.intents();
                let (metadata, payload) = intents.next().unwrap();
                assert_eq!(payload, (state + amount).to_le_bytes());
                assert_eq!(metadata.identity, MachineIntentId::from_bytes([id; 32]));
                assert_eq!(metadata.digest, penelope_core::task::Digest::of(payload).0);
                assert_eq!(metadata.payload_bytes, 8);
                assert!(intents.next().is_none());
            }
            let marker = directory.path().join(format!("marker-{id}"));
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "machine::tests::commit_process_helper",
                    "--nocapture",
                ])
                .env("PENELOPE_MACHINE_DB", &path)
                .env("PENELOPE_MACHINE_CHAIN_EVENT", format!("{amount},{id}"))
                .env("PENELOPE_MACHINE_CUT", phase)
                .env("PENELOPE_MACHINE_MARKER", &marker)
                .spawn()
                .unwrap();
            wait_for_marker(&mut child, &marker, phase);
            child.kill().unwrap();
            assert_eq!(child.wait().unwrap().signal(), Some(9));
            let committed = phase == "after-commit";
            if committed {
                expected.push(std::sync::Arc::clone(&request));
            }
            let reopened = support::open(&path);
            assert_killed_chain(
                &path,
                &reopened,
                &expected,
                state + if committed { amount } else { 0 },
            );
            let found =
                block_on(reopened.lookup(&true, SCOPE, MachineEventId::from_bytes([id; 32])))
                    .unwrap();
            assert_eq!(found.is_some(), committed);
            let receipt =
                block_on(reopened.commit(&true, std::sync::Arc::clone(&request))).unwrap();
            assert_eq!(
                receipt.disposition,
                if committed {
                    MachineCommitDisposition::AlreadyCommitted
                } else {
                    MachineCommitDisposition::Committed
                }
            );
            if let Some(found) = found {
                assert_eq!(receipt.identity, found.receipt().identity);
            } else {
                expected.push(request);
            }
            state += amount;
            drop(reopened);
            let recovered = support::open(&path);
            assert_killed_chain(&path, &recovered, &expected, state);
            println!("chain seed={seed} event={id} phase={phase} signal=9 exact_prefix=true");
        }
    }
}
