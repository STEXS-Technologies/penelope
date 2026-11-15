//! Actual process death while the production commit transaction is in flight.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use crate::{SqliteLimits, SqliteMachineStore};
use penelope_core::machine::*;
use penelope_ports::machine::*;
use std::{
    os::unix::process::ExitStatusExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
#[path = "../../tests/support/machines.rs"]
#[allow(dead_code)] // History fixture is used by the integration profile.
mod support;
use support::*;

pub(super) fn cut(phase: &str) {
    if std::env::var("PENELOPE_MACHINE_CUT").ok().as_deref() != Some(phase) {
        return;
    }
    std::fs::write(std::env::var_os("PENELOPE_MACHINE_MARKER").unwrap(), phase).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("parent did not kill commit child");
}
#[test]
fn commit_process_helper() {
    let Some(path) = std::env::var_os("PENELOPE_MACHINE_DB") else {
        return;
    };
    let store = support::open(Path::new(&path));
    block_on(store.commit(&true, batch(&mut owner(), 8, 4))).unwrap();
    panic!("commit cut not reached");
}
#[test]
fn sigkill_at_each_commit_boundary_recovers_all_or_nothing_and_resolves_lost_ack() {
    for phase in [
        "before-transaction",
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
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read(&marker).ok().as_deref() != Some(phase.as_bytes()) {
            if Instant::now() >= deadline {
                drop(child.kill());
                drop(child.wait());
                panic!("child failed to reach {phase}");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
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
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read(&marker).ok().as_deref() != Some(phase.as_bytes()) {
            if Instant::now() >= deadline {
                drop(child.kill());
                drop(child.wait());
                panic!("child failed to reach {phase}");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
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
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read(&marker).ok().as_deref() != Some(phase.as_bytes()) {
            if Instant::now() >= deadline {
                drop(child.kill());
                drop(child.wait());
                panic!("v7 migration failed to reach {phase}");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        child.kill().unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        let recovered_connection = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(rows(&recovered_connection), before, "{phase}");
        let version: u32 = recovered_connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            version,
            if phase == "after-commit" { 8 } else { 7 },
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
