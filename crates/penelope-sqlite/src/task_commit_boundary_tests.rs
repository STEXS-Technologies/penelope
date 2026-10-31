//! A failed COMMIT must not publish a task projection or retain staged blobs.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use super::*;
use crate::write_snapshot;
use rusqlite::functions::FunctionFlags;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
const TABLES: [&str; 5] = [
    "task_heads",
    "task_records",
    "task_blobs",
    "runtime_generations",
    "task_retirements",
];
const fn definition() -> TaskDefinition {
    TaskDefinition {
        code: Digest([3; 32]),
        checkpoint_schema: 1,
        effects: EffectContract::Pure,
        recovery: TaskRecoveryPolicy {
            mode: RecoveryMode::RestartFromCheckpoint,
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
            deadline_tick: TaskTick::new(10000),
        },
    }
}
fn scope(other: bool) -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([if other { 8 } else { 2 }; 32])),
        definition: definition().fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(b"input")),
    }
}
fn reference(bytes: &[u8]) -> BlobRef {
    BlobRef {
        digest: Digest::of(bytes),
        bytes: u32::try_from(bytes.len()).unwrap(),
        schema: 1,
    }
}
fn intent(projection: &TaskProjection) -> TaskRecord {
    projection
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition: definition(),
                input: reference(b"input"),
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        )
        .unwrap()
}
fn open_journal(path: &Path) -> SqliteTaskJournal {
    SqliteTaskJournal::open(path, definition(), SqliteLimits::default()).unwrap()
}
fn arm(journal: &SqliteTaskJournal) -> Arc<AtomicUsize> {
    assert!(
        journal
            .connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))
            .unwrap()
    );
    journal.connection.execute_batch("CREATE TEMP TABLE task_boundary_parent(value INTEGER PRIMARY KEY); CREATE TEMP TABLE task_boundary_child(value INTEGER REFERENCES task_boundary_parent(value) DEFERRABLE INITIALLY DEFERRED)").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    journal
        .connection
        .create_scalar_function(
            "task_boundary_stage",
            0,
            FunctionFlags::SQLITE_UTF8,
            move |_context| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(0_i64)
            },
        )
        .unwrap();
    journal.connection.execute_batch("CREATE TEMP TRIGGER task_boundary_fail AFTER INSERT ON main.task_records BEGIN INSERT INTO task_boundary_child VALUES(7); SELECT task_boundary_stage(); END").unwrap();
    calls
}
fn exercise(
    journal: &mut SqliteTaskJournal,
    projection: TaskProjection,
    record: TaskRecord,
    blob: Option<&[u8]>,
) {
    let before = write_snapshot::snapshot(&journal.connection, &TABLES);
    let cache_before = journal.cache;
    let calls = arm(journal);
    assert_eq!(journal.commit(&record, blob), Err(TaskPortError::Conflict));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the final record insert must finish before COMMIT fails"
    );
    assert!(journal.connection.is_autocommit());
    assert_eq!(
        journal.cache, cache_before,
        "a failed COMMIT must not publish a new cached frontier"
    );
    assert_eq!(
        write_snapshot::snapshot(&journal.connection, &TABLES),
        before
    );
    let children: i64 = journal
        .connection
        .query_row("SELECT count(*) FROM task_boundary_child", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(children, 0);
    // Repair the deferred constraint while keeping the same trigger installed.
    journal
        .connection
        .execute("INSERT INTO task_boundary_parent VALUES(7)", [])
        .unwrap();
    assert_eq!(journal.commit(&record, blob), Ok(record));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the repaired retry must traverse the same final insert"
    );
    assert!(journal.connection.is_autocommit());
    let mut expected = projection;
    expected.apply(&record).unwrap();
    assert_eq!(journal.cache.unwrap().0, expected);
    assert_eq!(
        journal.read(record.scope, record.sequence),
        Ok(Some(record))
    );
    let after = write_snapshot::snapshot(&journal.connection, &TABLES);
    assert_eq!(journal.commit(&record, blob), Err(TaskPortError::Conflict));
    assert_eq!(
        write_snapshot::snapshot(&journal.connection, &TABLES),
        after
    );
    if let Some(bytes) = blob {
        let mut decoded = [0_u8; 64];
        let size = journal
            .read_blob(record.scope, reference(bytes), &mut decoded)
            .unwrap();
        assert_eq!(decoded.get(..size), Some(bytes));
    }
}
fn cache_mode(journal: &mut SqliteTaskJournal, path: &Path, mode: &str) {
    match mode {
        "warm" => assert!(journal.cache.is_some()),
        "cold" => {
            *journal = open_journal(path);
            assert!(journal.cache.is_none());
        }
        "other" => {
            let projection = TaskProjection::new(scope(true), definition()).unwrap();
            journal
                .commit(&intent(&projection), Some(b"input"))
                .unwrap();
            assert_eq!(journal.cache.unwrap().0.scope(), scope(true));
        }
        _ => panic!("invalid cache fixture"),
    }
}
#[test]
fn deferred_task_commit_failure_preserves_cache_blobs_and_reopened_history() {
    let mut cases = 0;
    for mode in ["warm", "cold", "other"] {
        for kind in ["started", "new-checkpoint", "shared-checkpoint"] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("task-boundary.db");
            let mut journal = open_journal(&path);
            let mut projection = TaskProjection::new(scope(false), definition()).unwrap();
            let first = intent(&projection);
            journal.commit(&first, Some(b"input")).unwrap();
            projection.apply(&first).unwrap();
            let (event, blob) = if kind == "started" {
                (TaskExecutionEvent::Started, None)
            } else {
                let started = projection
                    .propose(
                        TaskExecutionEvent::Started,
                        TaskGeneration::new(1),
                        TaskTick::new(1),
                    )
                    .unwrap();
                journal.commit(&started, None).unwrap();
                projection.apply(&started).unwrap();
                let bytes = if kind == "new-checkpoint" {
                    b"checkpoint".as_slice()
                } else {
                    b"input".as_slice()
                };
                (
                    TaskExecutionEvent::CheckpointCommitted {
                        checkpoint: reference(bytes),
                    },
                    Some(bytes),
                )
            };
            let record = projection
                .propose(event, TaskGeneration::new(1), TaskTick::new(2))
                .unwrap();
            cache_mode(&mut journal, &path, mode);
            exercise(&mut journal, projection, record, blob);
            let after = write_snapshot::snapshot(&journal.connection, &TABLES);
            drop(journal);
            let mut reopened = open_journal(&path);
            assert_eq!(
                reopened.read(scope(false), TaskSequence::ZERO),
                Ok(Some(first))
            );
            assert_eq!(
                reopened.read(scope(false), record.sequence),
                Ok(Some(record))
            );
            assert_eq!(reopened.commit(&record, blob), Err(TaskPortError::Conflict));
            assert_eq!(
                write_snapshot::snapshot(&reopened.connection, &TABLES),
                after
            );
            cases += 1;
        }
    }
    assert_eq!(cases, 9);
    println!(
        "deferred_task_append_cases={cases} cache_preserved=true blob_rollback=true repaired_constraint_retry=true reopen=true"
    );
}
#[test]
fn deferred_first_intent_failure_cannot_publish_a_head_or_input_blob() {
    let mut cases = 0;
    for mode in ["cold", "other"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("intent-boundary.db");
        let mut journal = open_journal(&path);
        let projection = TaskProjection::new(scope(false), definition()).unwrap();
        let record = intent(&projection);
        cache_mode(&mut journal, &path, mode);
        exercise(&mut journal, projection, record, Some(b"input"));
        let after = write_snapshot::snapshot(&journal.connection, &TABLES);
        drop(journal);
        let mut reopened = open_journal(&path);
        assert_eq!(
            reopened.read(scope(false), TaskSequence::ZERO),
            Ok(Some(record))
        );
        assert_eq!(
            reopened.commit(&record, Some(b"input")),
            Err(TaskPortError::Conflict)
        );
        assert_eq!(
            write_snapshot::snapshot(&reopened.connection, &TABLES),
            after
        );
        cases += 1;
    }
    assert_eq!(cases, 2);
    println!(
        "deferred_first_intent_cases={cases} no_premature_head=true no_retained_blob=true repaired_constraint_retry=true reopen=true"
    );
}

fn scope_snapshot(journal: &SqliteTaskJournal, scope: TaskScope) -> write_snapshot::Snapshot {
    let identity = rusqlite::types::Value::Blob(key(scope).to_vec());
    write_snapshot::snapshot(&journal.connection, &TABLES)
        .into_iter()
        .map(|(table, rows)| {
            (
                table,
                rows.into_iter()
                    .filter(|row| row.first() == Some(&identity))
                    .collect(),
            )
        })
        .collect()
}
#[test]
fn deferred_retirement_failure_restores_history_then_preserves_reopened_tombstone() {
    let mut cases = 0;
    for mode in ["warm", "cold", "other"] {
        for identity in [None, Some(Digest([9; 32]))] {
            for shared_output in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("retirement-boundary.db");
                let mut journal = open_journal(&path);
                let other = TaskProjection::new(scope(true), definition()).unwrap();
                journal.commit(&intent(&other), Some(b"input")).unwrap();
                let mut projection = TaskProjection::new(scope(false), definition()).unwrap();
                let first = intent(&projection);
                journal.commit(&first, Some(b"input")).unwrap();
                projection.apply(&first).unwrap();
                let started = projection
                    .propose(
                        TaskExecutionEvent::Started,
                        TaskGeneration::new(1),
                        TaskTick::new(1),
                    )
                    .unwrap();
                journal.commit(&started, None).unwrap();
                projection.apply(&started).unwrap();
                let start = TaskStart {
                    scope: scope(false),
                    generation: projection.generation(),
                    definition: definition(),
                    execution_deadline: projection.execution_deadline(),
                    input_schema: 1,
                    input: b"input",
                    checkpoint: None,
                };
                if let Some(execution) = identity {
                    assert_eq!(
                        journal.reserve_start(start, execution),
                        Ok(StartReservation::Reserved)
                    );
                }
                let output = if shared_output {
                    b"input".as_slice()
                } else {
                    b"done".as_slice()
                };
                let complete = projection
                    .propose(
                        TaskExecutionEvent::Completed {
                            output: reference(output),
                        },
                        projection.generation(),
                        TaskTick::new(2),
                    )
                    .unwrap();
                journal.commit(&complete, Some(output)).unwrap();
                projection.apply(&complete).unwrap();
                assert_eq!(
                    journal.begin_fence(scope(false), projection.generation()),
                    Ok(identity)
                );
                journal
                    .confirm_stopped(scope(false), projection.generation(), identity)
                    .unwrap();
                match mode {
                    "warm" => assert_eq!(journal.cache.unwrap().0.scope(), scope(false)),
                    "cold" => journal = open_journal(&path),
                    "other" => journal.rebuild_head(scope(true)).unwrap(),
                    _ => panic!("invalid retirement cache mode"),
                }
                let before = write_snapshot::snapshot(&journal.connection, &TABLES);
                let unrelated = scope_snapshot(&journal, scope(true));
                journal.connection.execute_batch("CREATE TEMP TABLE retirement_parent(value INTEGER PRIMARY KEY); CREATE TEMP TABLE retirement_child(value INTEGER REFERENCES retirement_parent(value) DEFERRABLE INITIALLY DEFERRED)").unwrap();
                let calls = Arc::new(AtomicUsize::new(0));
                let observed = calls.clone();
                journal
                    .connection
                    .create_scalar_function(
                        "retirement_deleted",
                        3,
                        FunctionFlags::SQLITE_UTF8,
                        move |context| {
                            let empty = context.get::<i64>(0)? == 0
                                && context.get::<i64>(1)? == 0
                                && context.get::<i64>(2)? == 0;
                            observed.fetch_add(if empty { 1 } else { 10 }, Ordering::SeqCst);
                            Ok(0_i64)
                        },
                    )
                    .unwrap();
                journal.connection.execute_batch("CREATE TEMP TRIGGER retirement_commit_fail AFTER DELETE ON main.task_heads BEGIN INSERT INTO retirement_child VALUES(7); SELECT retirement_deleted((SELECT count(*) FROM task_records WHERE scope=OLD.scope),(SELECT count(*) FROM task_blobs WHERE scope=OLD.scope),(SELECT count(*) FROM task_heads WHERE scope=OLD.scope)); END").unwrap();
                assert_eq!(
                    journal.retire_task(scope(false), projection.next_sequence()),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(
                    calls.load(Ordering::SeqCst),
                    1,
                    "all target history must be deleted inside the failing transaction"
                );
                assert!(journal.connection.is_autocommit());
                assert!(journal.cache.is_none());
                assert_eq!(
                    write_snapshot::snapshot(&journal.connection, &TABLES),
                    before
                );
                let children: i64 = journal
                    .connection
                    .query_row("SELECT count(*) FROM retirement_child", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(children, 0);
                assert_eq!(
                    journal.read(scope(false), TaskSequence::ZERO),
                    Ok(Some(first))
                );
                assert_eq!(
                    journal.read(scope(false), complete.sequence),
                    Ok(Some(complete))
                );
                let mut restored = [0_u8; 64];
                let size = journal
                    .read_blob(scope(false), reference(output), &mut restored)
                    .unwrap();
                assert_eq!(restored.get(..size), Some(output));
                journal
                    .connection
                    .execute("INSERT INTO retirement_parent VALUES(7)", [])
                    .unwrap();
                let receipt = journal
                    .retire_task(scope(false), projection.next_sequence())
                    .unwrap();
                assert_eq!(calls.load(Ordering::SeqCst), 2);
                assert_eq!(receipt.scope, scope(false));
                assert_eq!(receipt.next_sequence, projection.next_sequence());
                assert_eq!(
                    receipt.last_record_digest,
                    Digest::of(&serde_json::to_vec(&complete).unwrap())
                );
                let tombstone: (u32, Vec<u8>) = journal
                    .connection
                    .query_row(
                        "SELECT next_sequence,last_digest FROM task_retirements WHERE scope=?1",
                        [key(scope(false))],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(
                    tombstone,
                    (
                        receipt.next_sequence.get(),
                        receipt.last_record_digest.0.to_vec()
                    )
                );
                assert_eq!(scope_snapshot(&journal, scope(true)), unrelated);
                let remaining:(i64,i64,i64) = journal.connection.query_row("SELECT (SELECT count(*) FROM task_records WHERE scope=?1),(SELECT count(*) FROM task_blobs WHERE scope=?1),(SELECT count(*) FROM task_heads WHERE scope=?1)",[key(scope(false))],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
                assert_eq!(remaining, (0, 0, 0));
                let authority: (Option<Vec<u8>>, i64) = journal
                    .connection
                    .query_row(
                        "SELECT execution,state FROM runtime_generations WHERE scope=?1",
                        [key(scope(false))],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(authority, (identity.map(|value| value.0.to_vec()), 2));
                let after = write_snapshot::snapshot(&journal.connection, &TABLES);
                drop(journal);
                let mut reopened = open_journal(&path);
                assert_eq!(
                    reopened.read(scope(false), TaskSequence::ZERO),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(
                    reopened.commit(&first, Some(b"input")),
                    Err(TaskPortError::Conflict)
                );
                assert!(reopened.reserve_start(start, Digest([9; 32])).is_err());
                assert_eq!(
                    reopened.retire_task(scope(false), projection.next_sequence()),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(
                    reopened.confirm_stopped(
                        scope(false),
                        projection.generation(),
                        Some(Digest([7; 32]))
                    ),
                    Err(TaskPortError::Conflict)
                );
                assert_eq!(
                    reopened.confirm_stopped(scope(false), projection.generation(), identity),
                    Ok(())
                );
                assert_eq!(
                    write_snapshot::snapshot(&reopened.connection, &TABLES),
                    after
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 12);
    println!(
        "deferred_retirement_cases={cases} staged_deletions_observed=true rollback=true same_trigger_retry=true unrelated_scope_preserved=true stopped_identity_preserved=true reopened_tombstone=true"
    );
}
