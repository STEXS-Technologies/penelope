//! Real-process allocation probe; setup and durable commits are outside counts.
use super::count;
use penelope_core::task::{runtime::*, *};
use penelope_sqlite::{SqliteLimits, SqliteTaskJournal};
use std::time::{Duration, Instant};

#[path = "runtime_fixture.rs"]
mod runtime_fixture;

pub(super) fn run() {
    let directory = tempfile::tempdir().unwrap();
    let definition = runtime_fixture::definition(2);
    let input = [5, 4, 5];
    let scope = TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition.fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(&input)),
    };
    let path = directory.path().join("tasks.db");
    let initial_journal =
        SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let initial_runtime =
        runtime_fixture::Runtime::new(authority, runtime_fixture::config(directory.path()))
            .unwrap();
    let mut supervisor = runtime_fixture::Supervisor::restore(
        initial_journal,
        initial_runtime,
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    supervisor.submit(&input, 1, TaskTick::ZERO).unwrap();
    supervisor.advance(TaskTick::new(1)).unwrap();
    let projection = *supervisor.projection();
    let generation = projection.generation();
    let (mut journal, mut runtime) = supervisor.into_parts();
    let mut buffer = [0; 64];
    let deadline = Instant::now().checked_add(Duration::from_secs(5)).unwrap();
    loop {
        let mut progress = RuntimeProgress::Pending;
        assert_eq!(
            count(|| {
                progress = runtime
                    .poll(scope, generation, &mut buffer)
                    .unwrap()
                    .progress;
            }),
            0
        );
        if let RuntimeProgress::Checkpoint { bytes, schema } = progress {
            let data = buffer.get(..usize::try_from(bytes).unwrap()).unwrap();
            let checkpoint = BlobRef {
                digest: Digest::of(data),
                bytes,
                schema,
            };
            let record = projection
                .propose(
                    TaskExecutionEvent::CheckpointCommitted { checkpoint },
                    generation,
                    TaskTick::new(2),
                )
                .unwrap();
            journal.commit(&record, Some(data)).unwrap();
            assert_eq!(
                count(|| runtime
                    .acknowledge_checkpoint(scope, generation, checkpoint)
                    .unwrap()),
                0
            );
            break;
        }
        assert_eq!(progress, RuntimeProgress::Pending);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    loop {
        let observation = runtime.poll(scope, generation, &mut buffer).unwrap();
        if matches!(
            observation.progress,
            RuntimeProgress::Complete {
                bytes: 8,
                schema: 1
            }
        ) {
            assert_eq!(buffer.get(..8).unwrap(), 9_u64.to_le_bytes());
            break;
        }
        assert_eq!(observation.progress, RuntimeProgress::Pending);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    runtime.fence(scope, generation).unwrap();
    let repeated_fences = std::array::from_fn::<_, 3, _>(|_sample| {
        count(|| runtime.fence(scope, generation).unwrap())
    });
    println!("Real runtime allocations: repeated public fences={repeated_fences:?}");
    println!("Real runtime allocations: pending/checkpoint poll=0, committed checkpoint ACK=0");
}
