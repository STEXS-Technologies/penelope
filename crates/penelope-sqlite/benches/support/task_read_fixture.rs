//! Single-record public task-journal read fixture.
#![allow(clippy::unwrap_used)]
use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
pub(super) fn run(
    mut sample: impl FnMut(&str, &mut dyn FnMut() -> Option<TaskRecord>, &Option<TaskRecord>),
) {
    let definition = TaskDefinition {
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
    };
    let scope = TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(b"input")),
    };
    let record = TaskProjection::new(scope, definition)
        .unwrap()
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition,
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
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("task-read.db");
    let mut journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    journal.commit(&record, Some(b"input")).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    for (name, padded, sequence) in [("record", false, 0), ("padded", true, 0), ("end", false, 1)] {
        let mut bytes = serde_json::to_vec(&record).unwrap();
        if padded {
            assert!(bytes.len() < 4096);
            bytes.resize(4096, b' ');
        }
        connection
            .execute(
                "UPDATE task_records SET record=?1,checksum=?2",
                rusqlite::params![bytes, Digest::of(&bytes).0.as_slice()],
            )
            .unwrap();
        let expected = (sequence == 0).then_some(record);
        let mut read = || journal.read(scope, TaskSequence::new(sequence)).unwrap();
        for _ in 0..100 {
            assert_eq!(read(), expected);
        }
        sample(name, &mut read, &expected);
        assert_eq!(read(), expected);
    }
}
