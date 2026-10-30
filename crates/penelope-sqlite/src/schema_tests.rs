//! Actual process death at transactional migration boundaries; no production hooks.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use super::*;
use crate::SqliteTaskJournal;
use penelope_core::task::{runtime::*, *};
use std::{os::unix::process::ExitStatusExt, path::Path, process::Command, time::Instant};

pub(super) fn cut(phase: &str) {
    if std::env::var("PENELOPE_MIGRATION_CUT").ok().as_deref() != Some(phase) {
        return;
    }
    std::fs::write(
        std::env::var_os("PENELOPE_MIGRATION_MARKER").unwrap(),
        phase,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(
            Instant::now() < deadline,
            "migration parent failed to kill child"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

const fn definition() -> TaskDefinition {
    TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromBeginning,
            max_attempts: 3,
            timeout_ticks: 50,
            restart_delay_ticks: 1,
            max_checkpoint_bytes: 64,
        },
        budget: ExecutionBudget {
            max_input_bytes: 64,
            max_output_bytes: 64,
            max_history_records: 32,
            max_checkpoint_commits: 8,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(1000),
        },
    }
}
fn scope() -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition().fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(b"input")),
    }
}
fn seed_v1(path: &Path) -> TaskProjection {
    let mut journal = SqliteTaskJournal::open(path, definition(), SqliteLimits::default()).unwrap();
    let mut projection = TaskProjection::new(scope(), definition()).unwrap();
    let intent = projection
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition: definition(),
                input: BlobRef {
                    digest: Digest::of(b"input"),
                    bytes: 5,
                    schema: 1,
                },
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        )
        .unwrap();
    journal.commit(&intent, Some(b"input")).unwrap();
    projection.apply(&intent).unwrap();
    let started = projection.next(TaskTick::new(1)).unwrap().unwrap();
    journal.commit(&started, None).unwrap();
    projection.apply(&started).unwrap();
    assert_eq!(
        journal
            .begin_fence(scope(), TaskGeneration::new(1))
            .unwrap(),
        None
    );
    journal
        .confirm_stopped(scope(), TaskGeneration::new(1), None)
        .unwrap();
    drop(journal);
    let connection = Connection::open(path).unwrap();
    connection.execute_batch("DROP TABLE task_retirements; DROP TABLE process_payloads; DROP TABLE process_usage; DROP TABLE process_heads; DROP TABLE process_outcomes; DROP TABLE process_inbox; DROP TABLE process_sources; DROP TABLE process_commits; DROP TABLE process_actions; DROP TABLE process_timers; DROP TABLE lease_counter; PRAGMA user_version=1;").unwrap();
    projection
}
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    records: Vec<Vec<u8>>,
    blobs: Vec<Vec<u8>>,
    authority: Vec<(i64, Option<Vec<u8>>, i64)>,
}
fn snapshot(connection: &Connection) -> Snapshot {
    let records = connection
        .prepare("SELECT record FROM task_records ORDER BY sequence")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let blobs = connection
        .prepare("SELECT bytes FROM task_blobs ORDER BY digest")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let authority = connection
        .prepare("SELECT generation,execution,state FROM runtime_generations ORDER BY generation")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    Snapshot {
        records,
        blobs,
        authority,
    }
}

#[test]
fn migration_process_helper() {
    let Some(path) = std::env::var_os("PENELOPE_MIGRATION_DB") else {
        return;
    };
    drop(SqliteTaskJournal::open(Path::new(&path), definition(), SqliteLimits::default()).unwrap());
    panic!("migration cut was not reached");
}

#[test]
fn sigkill_at_schema_migration_boundaries_preserves_records_blobs_and_fences() {
    for phase in [
        "before-transaction",
        "after-begin",
        "after-v2",
        "after-v3",
        "after-v4",
        "before-commit",
        "after-commit",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tasks.db");
        let expected_projection = seed_v1(&path);
        let original = snapshot(&Connection::open(&path).unwrap());
        let marker = directory.path().join("marker");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "schema::tests::migration_process_helper",
                "--nocapture",
            ])
            .env("PENELOPE_MIGRATION_DB", &path)
            .env("PENELOPE_MIGRATION_MARKER", &marker)
            .env("PENELOPE_MIGRATION_CUT", phase)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while std::fs::read(&marker).ok().as_deref() != Some(phase.as_bytes()) {
            if Instant::now() >= deadline {
                drop(child.kill());
                drop(child.wait());
                panic!("migration failed to reach {phase}");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        child.kill().unwrap();
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        let connection = Connection::open(&path).unwrap();
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            version,
            if phase == "after-commit" { 4 } else { 1 },
            "{phase}"
        );
        assert_eq!(snapshot(&connection), original, "{phase}");
        for table in ["task_retirements", "process_heads", "process_payloads"] {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, phase == "after-commit", "{phase}: {table}");
        }
        drop(connection);
        let mut upgraded =
            SqliteTaskJournal::open(&path, definition(), SqliteLimits::default()).unwrap();
        let mut replay = TaskProjection::new(scope(), definition()).unwrap();
        for sequence in 0..2 {
            replay
                .apply(
                    &upgraded
                        .read(scope(), TaskSequence::new(sequence))
                        .unwrap()
                        .unwrap(),
                )
                .unwrap();
        }
        assert_eq!(replay, expected_projection);
        assert_eq!(upgraded.read(scope(), TaskSequence::new(2)).unwrap(), None);
        let mut input = [0; 5];
        assert_eq!(
            upgraded
                .read_blob(scope(), replay.input().unwrap(), &mut input)
                .unwrap(),
            5
        );
        assert_eq!(input, *b"input");
        let request = TaskStart {
            scope: scope(),
            generation: TaskGeneration::new(1),
            definition: definition(),
            execution_deadline: replay.execution_deadline(),
            input_schema: 1,
            input: b"input",
            checkpoint: None,
        };
        assert_eq!(
            upgraded.reserve_start(request, Digest([9; 32])),
            Err(TaskPortError::Conflict)
        );
        drop(upgraded);
        let checked_connection = Connection::open(&path).unwrap();
        assert_eq!(snapshot(&checked_connection), original);
        assert_eq!(
            checked_connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            4
        );
    }
}
