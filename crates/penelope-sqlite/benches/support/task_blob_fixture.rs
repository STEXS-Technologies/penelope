//! Caller-buffer blob reads, healthy and corrupt, through the public task port.
#![allow(clippy::unwrap_used)]
use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
pub(super) struct Fixture {
    journal: SqliteTaskJournal,
    connection: rusqlite::Connection,
    scope: TaskScope,
    reference: BlobRef,
    buffer: Vec<u8>,
    stored: Vec<u8>,
    pub(super) size: u32,
    pub(super) corrupt: bool,
}
impl Fixture {
    pub(super) fn read(&mut self) -> Result<usize, TaskPortError> {
        self.journal
            .read_blob(self.scope, self.reference, &mut self.buffer)
    }
    pub(super) fn verify(&self, result: Result<usize, TaskPortError>) {
        if self.corrupt {
            assert_eq!(result, Err(TaskPortError::Corrupt));
            assert!(self.buffer.iter().all(|b| *b == 0xa5));
        } else {
            assert_eq!(result, Ok(usize::try_from(self.size).unwrap()));
            assert!(self.buffer.iter().all(|b| *b == 7));
        }
    }
}
pub(super) fn run(mut sample: impl FnMut(&mut Fixture)) {
    for size in [0_u32, 32, 4096, 1048576] {
        for corrupt in [false, true] {
            let bytes = vec![7; usize::try_from(size).unwrap()];
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
                    max_input_bytes: 1048576,
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
                input: TaskInputDigest::from_digest(Digest::of(&bytes)),
            };
            let reference = BlobRef {
                digest: Digest::of(&bytes),
                bytes: size,
                schema: 1,
            };
            let record = TaskProjection::new(scope, definition)
                .unwrap()
                .propose(
                    TaskExecutionEvent::IntentCommitted {
                        definition,
                        input: reference,
                    },
                    TaskGeneration::ZERO,
                    TaskTick::ZERO,
                )
                .unwrap();
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("blob.db");
            let mut journal =
                SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
            journal.commit(&record, Some(&bytes)).unwrap();
            let connection = rusqlite::Connection::open(&path).unwrap();
            let stored = if corrupt {
                vec![8; bytes.len().max(1)]
            } else {
                bytes
            };
            connection
                .execute("UPDATE task_blobs SET bytes=?1", [&stored])
                .unwrap();
            let mut fixture = Fixture {
                journal,
                connection,
                scope,
                reference,
                buffer: vec![0xa5; usize::try_from(size).unwrap()],
                stored,
                size,
                corrupt,
            };
            for _ in 0..16 {
                let result = fixture.read();
                fixture.verify(result);
            }
            sample(&mut fixture);
            let retained: Vec<u8> = fixture
                .connection
                .query_row("SELECT bytes FROM task_blobs", [], |row| row.get(0))
                .unwrap();
            assert_eq!(retained, fixture.stored);
            // Close connections before removing the temporary directory.
            drop(fixture);
            drop(directory);
        }
    }
}
