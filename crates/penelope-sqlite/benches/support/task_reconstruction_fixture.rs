//! Public exact start-reservation retry reconstructs the complete durable history.
#![allow(clippy::unwrap_used)]
use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
pub(super) fn run(
    mut sample: impl FnMut(u32, bool, bool, usize, &mut dyn FnMut() -> StartReservation),
) {
    let alternating = std::env::var_os("PENELOPE_BENCH_ALTERNATING_BLOBS").is_some();
    let reformatted = std::env::var_os("PENELOPE_BENCH_REFORMATTED_SCOPE").is_some();
    for count in [2_u32, 16, 128] {
        let definition = TaskDefinition {
            code: Digest([3; 32]),
            checkpoint_schema: 1,
            effects: EffectContract::Pure,
            recovery: TaskRecoveryPolicy {
                mode: RecoveryMode::RestartFromCheckpoint,
                max_attempts: 3,
                timeout_ticks: 1000,
                restart_delay_ticks: 1,
                max_checkpoint_bytes: 64,
            },
            budget: ExecutionBudget {
                max_input_bytes: 64,
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
            input: TaskInputDigest::from_digest(Digest::of(b"input")),
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reconstruct.db");
        let mut journal =
            SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
        let mut projection = TaskProjection::new(scope, definition).unwrap();
        let mut removed_bytes = 0_usize;
        let mut latest_checkpoint = None;
        for sequence in 0..count {
            let checkpoint_bytes = if alternating && sequence & 1 == 1 {
                b"other".as_slice()
            } else {
                b"state".as_slice()
            };
            if sequence >= 2 {
                latest_checkpoint = Some(checkpoint_bytes);
            }
            let (event, blob) = match sequence {
                0 => (
                    TaskExecutionEvent::IntentCommitted {
                        definition,
                        input: BlobRef {
                            digest: Digest::of(b"input"),
                            bytes: 5,
                            schema: 1,
                        },
                    },
                    Some(b"input".as_slice()),
                ),
                1 => (TaskExecutionEvent::Started, None),
                _ => (
                    TaskExecutionEvent::CheckpointCommitted {
                        checkpoint: BlobRef {
                            digest: Digest::of(checkpoint_bytes),
                            bytes: 5,
                            schema: 1,
                        },
                    },
                    Some(checkpoint_bytes),
                ),
            };
            let generation = if sequence == 0 {
                TaskGeneration::ZERO
            } else {
                TaskGeneration::new(1)
            };
            let record = projection
                .propose(event, generation, TaskTick::new(u64::from(sequence)))
                .unwrap();
            journal.commit(&record, blob).unwrap();
            projection.apply(&record).unwrap();
            removed_bytes = removed_bytes
                .checked_add(serde_json::to_vec(&record).unwrap().len())
                .and_then(|n| n.checked_add(32))
                .unwrap();
        }
        if reformatted {
            let mut connection = rusqlite::Connection::open(&path).unwrap();
            let records: Vec<(u32, Vec<u8>)> = connection
                .prepare("SELECT sequence,record FROM task_records ORDER BY sequence")
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let transaction = connection.transaction().unwrap();
            removed_bytes = 0;
            for (sequence, encoded) in records {
                let record: TaskRecord = serde_json::from_slice(&encoded).unwrap();
                let pretty = serde_json::to_vec_pretty(&record).unwrap();
                removed_bytes = removed_bytes
                    .checked_add(pretty.len())
                    .and_then(|n| n.checked_add(32))
                    .unwrap();
                transaction
                    .execute(
                        "UPDATE task_records SET record=?1,checksum=?2 WHERE sequence=?3",
                        rusqlite::params![&pretty, Digest::of(&pretty).0.as_slice(), sequence],
                    )
                    .unwrap();
            }
            transaction.commit().unwrap();
            journal.rebuild_head(scope).unwrap();
        }
        let request = TaskStart {
            scope,
            generation: projection.generation(),
            definition,
            execution_deadline: projection.execution_deadline(),
            input_schema: 1,
            input: b"input",
            checkpoint: latest_checkpoint,
        };
        let execution = Digest([9; 32]);
        assert_eq!(
            journal.reserve_start(request, execution).unwrap(),
            StartReservation::Reserved
        );
        let mut retry = || journal.reserve_start(request, execution).unwrap();
        for _ in 0..16 {
            assert_eq!(retry(), StartReservation::AlreadyReserved);
        }
        sample(count, alternating, reformatted, removed_bytes, &mut retry);
        assert_eq!(retry(), StartReservation::AlreadyReserved);
        drop(journal);
        let connection = rusqlite::Connection::open(&path).unwrap();
        let retained: u32 = connection
            .query_row("SELECT count(*) FROM task_records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(retained, count);
        let reservations: u32 = connection
            .query_row("SELECT count(*) FROM runtime_generations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(reservations, 1);
    }
}
