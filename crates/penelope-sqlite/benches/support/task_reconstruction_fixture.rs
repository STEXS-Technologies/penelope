//! Public exact start-reservation retry reconstructs the complete durable history.
#![allow(clippy::unwrap_used)]
use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
pub(super) fn run(mut sample: impl FnMut(u32, usize, &mut dyn FnMut() -> StartReservation)) {
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
        for sequence in 0..count {
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
                            digest: Digest::of(b"state"),
                            bytes: 5,
                            schema: 1,
                        },
                    },
                    Some(b"state".as_slice()),
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
        let request = TaskStart {
            scope,
            generation: projection.generation(),
            definition,
            execution_deadline: projection.execution_deadline(),
            input_schema: 1,
            input: b"input",
            checkpoint: (count > 2).then_some(b"state".as_slice()),
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
        sample(count, removed_bytes, &mut retry);
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
