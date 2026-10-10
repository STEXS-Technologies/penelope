//! Process termination through a real transactional database, not simulated I/O.
//! This exercises process-death recovery; it does not certify power-loss safety.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    clippy::wildcard_enum_match_arm
)]
#[path = "support/sqlite_journal.rs"]
mod sqlite_journal;
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::{runtime::*, *};
use penelope_executor::task::TaskSupervisor;
use sqlite_journal::SqliteJournal;
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use support::{SumRuntime, definition, scope};

#[test]
fn sqlite_crash_worker() {
    let Ok(root) = std::env::var("PENELOPE_SQLITE_CRASH_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let boundary = match std::env::var("PENELOPE_SQLITE_CRASH_BOUNDARY")
        .unwrap()
        .as_str()
    {
        "after-blob-insert" => "after-blob-insert",
        "before-commit" => "before-commit",
        "after-commit" => "after-commit",
        _ => panic!("invalid test boundary"),
    };
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &[1; 64]);
    let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
        SqliteJournal::open(&root.join("journal.sqlite"), definition),
        SumRuntime::default(),
        scope,
        definition,
        TaskTick::new(0),
    )
    .unwrap();
    supervisor.submit(&[1; 64], 1, TaskTick::new(0)).unwrap();
    supervisor.advance(TaskTick::new(1)).unwrap();
    let (mut journal, runtime) = supervisor.into_parts();
    journal.pause = Some((boundary, root.join("ready")));
    let mut restarted_supervisor =
        TaskSupervisor::<_, _, 64>::restore(journal, runtime, scope, definition, TaskTick::new(2))
            .unwrap();
    // Restore fences the interrupted generation; authorize generation two, then
    // let the computation fixture propose its checkpoint through supervisor.poll.
    restarted_supervisor.advance(TaskTick::new(3)).unwrap();
    restarted_supervisor.advance(TaskTick::new(4)).unwrap();
    restarted_supervisor.poll(TaskTick::new(5)).unwrap();
    panic!("worker should have been terminated");
}

#[test]
fn process_death_rolls_back_or_recovers_the_whole_checkpoint_transaction() {
    for boundary in ["after-blob-insert", "before-commit", "after-commit"] {
        let root =
            std::env::temp_dir().join(format!("penelope-sqlite-{}-{boundary}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sqlite_crash_worker", "--nocapture"])
            .env("PENELOPE_SQLITE_CRASH_ROOT", &root)
            .env("PENELOPE_SQLITE_CRASH_BOUNDARY", boundary)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !root.join("ready").exists() {
            assert!(
                child.try_wait().unwrap().is_none(),
                "worker exited before fault boundary"
            );
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("worker did not reach fault boundary");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
        let scope = scope(definition, &[1; 64]);
        let journal = SqliteJournal::open(&root.join("journal.sqlite"), definition);
        let checkpoints: u32 = journal
            .connection
            .query_row(
                "SELECT COUNT(*) FROM blobs WHERE length(bytes)=12",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(checkpoints, u32::from(boundary == "after-commit"));
        let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
            journal,
            SumRuntime::default(),
            scope,
            definition,
            TaskTick::new(6),
        )
        .unwrap();
        assert_eq!(supervisor.projection().status(), TaskStatus::Faulted);
        assert_eq!(
            supervisor.projection().checkpoint().is_some(),
            boundary == "after-commit"
        );
        for tick in 7..40 {
            match supervisor.projection().status() {
                TaskStatus::Completed => break,
                TaskStatus::Running => {
                    supervisor.poll(TaskTick::new(tick)).unwrap();
                }
                _ => {
                    supervisor.advance(TaskTick::new(tick)).unwrap();
                }
            }
        }
        assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
        let output = supervisor.projection().output().unwrap();
        let (mut completed_journal, runtime) = supervisor.into_parts();
        assert_eq!(
            runtime.resumed,
            [if boundary == "after-commit" { 8 } else { 0 }]
        );
        let mut bytes = [0; 64];
        let length = completed_journal
            .read_blob(scope, output, &mut bytes)
            .unwrap();
        assert_eq!(&bytes[..length], &64_u64.to_le_bytes());
        drop(completed_journal);
        // A second connection reconstructs only the durable completion and never starts code.
        let restored = TaskSupervisor::<_, _, 64>::restore(
            SqliteJournal::open(&root.join("journal.sqlite"), definition),
            SumRuntime::default(),
            scope,
            definition,
            TaskTick::new(40),
        )
        .unwrap();
        assert_eq!(restored.projection().status(), TaskStatus::Completed);
        assert_eq!(restored.into_parts().1.starts, 0);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn separate_database_connections_arbitrate_one_intent_and_reject_corruption() {
    let root = std::env::temp_dir().join(format!("penelope-sqlite-cas-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &[1; 64]);
    let first = SqliteJournal::open(&root.join("journal.sqlite"), definition);
    let second = SqliteJournal::open(&root.join("journal.sqlite"), definition);
    let record = TaskProjection::new(scope, definition)
        .unwrap()
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition,
                input: BlobRef {
                    digest: Digest::of(&[1; 64]),
                    bytes: 64,
                    schema: 1,
                },
            },
            TaskGeneration::new(0),
            TaskTick::new(0),
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let results = std::thread::scope(|threads| {
        let handles: Vec<_> = [first, second]
            .into_iter()
            .map(|mut journal| {
                let barrier = barrier.clone();
                threads.spawn(move || {
                    barrier.wait();
                    journal.commit(&record, Some(&[1; 64]))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(TaskPortError::Conflict))
            .count(),
        1
    );
    let mut journal = SqliteJournal::open(&root.join("journal.sqlite"), definition);
    assert_eq!(
        journal.read(scope, TaskSequence::new(0)).unwrap(),
        Some(record)
    );
    let wrong_scope = TaskScope {
        task: TaskId::from_digest(Digest([9; 32])),
        ..scope
    };
    let blob = BlobRef {
        digest: Digest::of(&[1; 64]),
        bytes: 64,
        schema: 1,
    };
    assert_eq!(
        journal.read_blob(wrong_scope, blob, &mut [0; 64]),
        Err(TaskPortError::Corrupt)
    );
    journal
        .connection
        .execute("UPDATE blobs SET bytes=zeroblob(64)", [])
        .unwrap();
    assert_eq!(
        journal.read_blob(scope, blob, &mut [0; 64]),
        Err(TaskPortError::Corrupt)
    );
    let mut altered = record;
    altered.tick = TaskTick::new(9);
    journal
        .connection
        .execute(
            "UPDATE records SET record=?1",
            [serde_json::to_vec(&altered).unwrap()],
        )
        .unwrap();
    assert_eq!(
        journal.read(scope, TaskSequence::new(0)),
        Err(TaskPortError::Corrupt)
    );
    drop(journal);
    fs::remove_dir_all(root).unwrap();
}
