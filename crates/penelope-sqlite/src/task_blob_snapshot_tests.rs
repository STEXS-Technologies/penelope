//! Gate a real external retirement inside the reader's tombstone query.
#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;
use rusqlite::functions::FunctionFlags;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
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
            deadline_tick: TaskTick::new(10000),
        },
    }
}
#[test]
fn blob_read_and_external_retirement_share_one_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("blob-retirement.db");
    let definition = definition();
    let scope = TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(b"input")),
    };
    let mut writer = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let mut projection = TaskProjection::new(scope, definition).unwrap();
    let input = BlobRef {
        digest: Digest::of(b"input"),
        bytes: 5,
        schema: 1,
    };
    let output = BlobRef {
        digest: Digest::of(b"done"),
        bytes: 4,
        schema: 1,
    };
    for (event, blob, generation, tick) in [
        (
            TaskExecutionEvent::IntentCommitted { definition, input },
            Some(b"input".as_slice()),
            TaskGeneration::ZERO,
            0,
        ),
        (TaskExecutionEvent::Started, None, TaskGeneration::new(1), 1),
        (
            TaskExecutionEvent::Completed { output },
            Some(b"done".as_slice()),
            TaskGeneration::new(1),
            2,
        ),
    ] {
        let record = projection
            .propose(event, generation, TaskTick::new(tick))
            .unwrap();
        writer.commit(&record, blob).unwrap();
        projection.apply(&record).unwrap();
    }
    let mut reader = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    assert_eq!(reader.read_blob(scope, output, &mut [0; 4]), Ok(4));
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let gate_calls = calls.clone();
    reader
        .connection
        .create_scalar_function(
            "retirement_gate",
            0,
            FunctionFlags::SQLITE_UTF8,
            move |_context| {
                gate_calls.fetch_add(1, Ordering::SeqCst);
                entered
                    .send(())
                    .map_err(|_closed| rusqlite::Error::InvalidQuery)?;
                resume
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_timeout| rusqlite::Error::InvalidQuery)?;
                Ok(Vec::<u8>::new())
            },
        )
        .unwrap();
    // The first branch establishes a real main-database snapshot. The second
    // returns only a nonmatching empty key after the external writer commits.
    reader.connection.execute_batch("CREATE TEMP VIEW task_retirements AS SELECT scope FROM main.task_retirements UNION ALL SELECT retirement_gate() AS scope").unwrap();
    let frontier = projection.next_sequence();
    let retiring = std::thread::spawn(move || {
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        let retired = writer.retire_task(scope, frontier).unwrap();
        release.send(()).unwrap();
        retired
    });
    let mut buffer = [0xa5; 8];
    let result = reader.read_blob(scope, output, &mut buffer);
    let retired = retiring.join().unwrap();
    assert_eq!(retired.scope, scope);
    assert_eq!(retired.next_sequence, frontier);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        result,
        Ok(4),
        "a read pinned before retirement must return its verified blob"
    );
    assert_eq!(&buffer[..4], b"done");
    assert_eq!(&buffer[4..], &[0xa5; 4]);
    let mut after = [0xa5; 8];
    assert_eq!(
        reader.read_blob(scope, output, &mut after),
        Err(TaskPortError::Conflict)
    );
    assert_eq!(after, [0xa5; 8]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let connection = Connection::open(&path).unwrap();
    let (records, blobs, heads, tombstones): (u32,u32,u32,u32) = connection.query_row(
        "SELECT (SELECT count(*) FROM task_records),(SELECT count(*) FROM task_blobs),(SELECT count(*) FROM task_heads),(SELECT count(*) FROM task_retirements)", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))
    ).unwrap();
    assert_eq!((records, blobs, heads, tombstones), (0, 0, 0, 1));
    let mut reopened = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    assert_eq!(
        reopened.read_blob(scope, output, &mut after),
        Err(TaskPortError::Conflict)
    );
    assert_eq!(after, [0xa5; 8]);
    println!(
        "retirement_committed_during_read=true pinned_blob=true later_conflict=true reopened=true"
    );
}
