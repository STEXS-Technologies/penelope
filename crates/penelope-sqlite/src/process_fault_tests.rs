//! Process death at the real saga transaction's write boundaries.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use super::*;
use penelope_domain::*;
use rusqlite::functions::FunctionFlags;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    str::FromStr,
    time::{Duration, Instant},
};
fn id<T: FromStr>(value: &str) -> T {
    value.parse().ok().unwrap()
}
fn scope() -> ProcessScope {
    ProcessScope::new(
        id("tnt_crash"),
        id("prc_crash"),
        id("def_crash"),
        id("dfv_one"),
        ContentDigest([9; 32]),
    )
}
fn commit() -> AtomicProcessCommit {
    let scope = scope();
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id("inp_crash"),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([1; 32]),
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            id("out_crash"),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::Started,
            ContentDigest([2; 32]),
        ),
    );
    let action = ProcessAction::new(
        scope,
        id("act_crash"),
        id("stp_crash"),
        0,
        ProcessActionKind::ExternalEffect,
        ContentDigest([3; 32]),
    );
    AtomicProcessCommit::new(0, Some(input), vec![outcome], vec![action])
        .unwrap()
        .with_canonical_source_event(id("cev_crash"))
}
fn pause(marker: &Path) -> ! {
    std::fs::write(marker, b"paused").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn crash_worker() {
    let Ok(path) = std::env::var("PENELOPE_PROCESS_CRASH_DB") else {
        return;
    };
    let marker = PathBuf::from(std::env::var("PENELOPE_PROCESS_CRASH_MARKER").unwrap());
    let boundary = std::env::var("PENELOPE_PROCESS_CRASH_BOUNDARY").unwrap();
    let limits = SqliteLimits::default();
    let mut connection = open(Path::new(&path), limits).unwrap();
    if boundary != "postcommit" {
        let trigger = match boundary.as_str() {
            "preinput" => {
                "CREATE TEMP TRIGGER pause_write BEFORE INSERT ON process_inbox BEGIN SELECT pause_test(); END;"
            }
            "postinput" => {
                "CREATE TEMP TRIGGER pause_write AFTER INSERT ON process_inbox BEGIN SELECT pause_test(); END;"
            }
            "postoutcome" => {
                "CREATE TEMP TRIGGER pause_write AFTER INSERT ON process_outcomes BEGIN SELECT pause_test(); END;"
            }
            "postaction" => {
                "CREATE TEMP TRIGGER pause_write AFTER INSERT ON process_actions BEGIN SELECT pause_test(); END;"
            }
            "precommit" => {
                "CREATE TEMP TRIGGER pause_write AFTER INSERT ON process_commits BEGIN SELECT pause_test(); END;"
            }
            _ => panic!("unknown process crash boundary"),
        };
        let marker = marker.clone();
        connection
            .create_scalar_function(
                "pause_test",
                0,
                FunctionFlags::SQLITE_UTF8,
                move |_context| -> rusqlite::Result<i32> { pause(&marker) },
            )
            .unwrap();
        connection.execute_batch(trigger).unwrap();
    }
    commit_process(&mut connection, &commit(), limits).unwrap();
    pause(&marker);
}
#[test]
fn process_death_never_splits_atomic_saga_commit() {
    for boundary in [
        "preinput",
        "postinput",
        "postoutcome",
        "postaction",
        "precommit",
        "postcommit",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("process.db");
        let marker = directory.path().join("marker");
        let limits = SqliteLimits::default();
        drop(open(&path, limits).unwrap());
        let mut worker = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process::fault_tests::crash_worker",
                "--nocapture",
            ])
            .env("PENELOPE_PROCESS_CRASH_DB", &path)
            .env("PENELOPE_PROCESS_CRASH_MARKER", &marker)
            .env("PENELOPE_PROCESS_CRASH_BOUNDARY", boundary)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.exists() {
            if Instant::now() >= deadline {
                let _kill = worker.kill();
                let _wait = worker.wait();
                panic!("worker did not reach {boundary}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        worker.kill().unwrap();
        worker.wait().unwrap();
        let mut reopened = open(&path, limits).unwrap();
        for table in [
            "process_inbox",
            "process_outcomes",
            "process_actions",
            "process_heads",
            "process_sources",
            "process_commits",
            "process_usage",
        ] {
            let count: u32 = reopened
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(
                count,
                u32::from(boundary == "postcommit"),
                "{boundary}: {table}"
            );
        }
        let receipt = commit_process(&mut reopened, &commit(), limits).unwrap();
        assert_eq!(receipt.duplicate_input, boundary == "postcommit");
        receipt.validate_for(&commit()).unwrap();
        let count: u32 = reopened
            .query_row("SELECT count(*) FROM process_outcomes", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}
