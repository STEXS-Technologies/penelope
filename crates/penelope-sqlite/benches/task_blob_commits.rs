//! WAL/FULL commits reusing an existing input blob as a checkpoint.
#![allow(clippy::unwrap_used)]
use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
fn main() {
    let external_commit = std::env::var_os("PENELOPE_BENCH_EXTERNAL_COMMIT").is_some();
    for size in [0_u32, 32, 4096, 1048576] {
        if external_commit && size > 4096 {
            continue;
        }
        let bytes = vec![7; usize::try_from(size).unwrap()];
        let definition = TaskDefinition {
            code: Digest([3; 32]),
            checkpoint_schema: 1,
            effects: EffectContract::Pure,
            recovery: TaskRecoveryPolicy {
                mode: RecoveryMode::RestartFromCheckpoint,
                max_attempts: 3,
                timeout_ticks: 1000,
                restart_delay_ticks: 1,
                max_checkpoint_bytes: 1048576,
            },
            budget: ExecutionBudget {
                max_input_bytes: 1048576,
                max_output_bytes: 64,
                max_history_records: 256,
                max_checkpoint_commits: 128,
                cpu_ticks: 10,
                deadline_tick: TaskTick::new(10000),
            },
        };
        let scope = TaskScope {
            tenant: TaskTenantId::from_digest(Digest([1; 32])),
            task: TaskId::from_digest(Digest([2; 32])),
            definition: definition.fingerprint(),
            input: TaskInputDigest::from_digest(Digest::of(&bytes)),
        };
        let reference = BlobRef {
            digest: Digest::of(&bytes),
            bytes: size,
            schema: 1,
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("duplicate.db");
        let mut journal =
            SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
        let mut projection = TaskProjection::new(scope, definition).unwrap();
        for sequence in 0..2_u32 {
            let (event, blob) = if sequence == 0 {
                (
                    TaskExecutionEvent::IntentCommitted {
                        definition,
                        input: reference,
                    },
                    Some(bytes.as_slice()),
                )
            } else {
                (TaskExecutionEvent::Started, None)
            };
            let generation = if sequence == 0 {
                TaskGeneration::ZERO
            } else {
                TaskGeneration::new(1)
            };
            let record = projection
                .propose(event, generation, TaskTick::new(u64::from(sequence)))
                .unwrap();
            assert_eq!(journal.commit(&record, blob).unwrap(), record);
            projection.apply(&record).unwrap();
        }
        let noise = external_commit.then(|| {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection
                .execute("CREATE TABLE cache_revision_noise(value INTEGER)", [])
                .unwrap();
            connection
        });
        let mut elapsed_ns = 0_u128;
        for sequence in 2..130_u32 {
            let record = projection
                .propose(
                    TaskExecutionEvent::CheckpointCommitted {
                        checkpoint: reference,
                    },
                    projection.generation(),
                    TaskTick::new(u64::from(sequence)),
                )
                .unwrap();
            // This unrelated external commit is deliberately outside timing;
            // any resulting journal replay remains inside the timed operation.
            if let Some(connection) = &noise {
                connection
                    .execute("INSERT INTO cache_revision_noise VALUES (?1)", [sequence])
                    .unwrap();
            }
            let started = std::time::Instant::now();
            let result = std::hint::black_box(journal.commit(&record, Some(&bytes)));
            elapsed_ns = elapsed_ns
                .checked_add(started.elapsed().as_nanos())
                .unwrap();
            assert_eq!(result.unwrap(), record);
            projection.apply(&record).unwrap();
        }
        if let Some(connection) = &noise {
            let count: u32 = connection
                .query_row("SELECT count(*) FROM cache_revision_noise", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 128);
        }
        drop(noise);
        journal.rebuild_head(scope).unwrap();
        drop(journal);
        let connection = rusqlite::Connection::open(&path).unwrap();
        let (records, blobs): (u32, u32) = connection
            .query_row(
                "SELECT (SELECT count(*) FROM task_records),(SELECT count(*) FROM task_blobs)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((records, blobs), (130, 1));
        drop(connection);
        println!(
            "{}",
            serde_json::json!({"size":size,"iterations":128,"elapsed_ns":elapsed_ns,"external_commit":external_commit})
        );
    }
}
