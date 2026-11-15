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
