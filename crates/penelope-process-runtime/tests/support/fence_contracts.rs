//! Damaged launch evidence and lock contention must never acknowledge a fence.
use super::*;
use rustix::fs::{FlockOperation, flock};
use std::os::unix::fs::symlink;

fn manifest(directory: &Path) -> PathBuf {
    std::fs::read_dir(directory.join("registry"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().is_some_and(|name| name.len() == 64))
        .unwrap()
}

#[test]
fn each_registry_binding_and_partial_identity_blocks_fence_until_repaired() {
    // Version, execution digest, full-scope digest, generation, definition,
    // reserved padding, partial PID/start time, truncated/oversized evidence.
    for damage in 0..10 {
        let directory = tempfile::tempdir().unwrap();
        let mut supervisor = fresh(directory.path(), &[9], 2);
        supervisor.advance(TaskTick::new(1)).unwrap();
        let projection = *supervisor.projection();
        let (_journal, mut runtime) = supervisor.into_parts();
        let manifest = manifest(directory.path());
        let saved = std::fs::read(&manifest).unwrap();
        let mut altered = saved.clone();
        match damage {
            0 => *altered.get_mut(0).unwrap() ^= 1,
            1 => *altered.get_mut(8).unwrap() ^= 1,
            2 => *altered.get_mut(40).unwrap() ^= 1,
            3 => *altered.get_mut(72).unwrap() ^= 1,
            4 => *altered.get_mut(76).unwrap() ^= 1,
            5 => *altered.get_mut(140).unwrap() = 1,
            6 => altered.get_mut(160..164).unwrap().fill(0),
            7 => altered.get_mut(164..172).unwrap().fill(0),
            8 => {
                altered.pop();
            }
            _ => altered.push(0),
        }
        std::fs::write(&manifest, altered).unwrap();
        assert_eq!(
            runtime.fence(projection.scope(), projection.generation()),
            Err(TaskPortError::Corrupt),
            "damage {damage}"
        );
        assert_eq!(
            runtime
                .poll(projection.scope(), projection.generation(), &mut [0; 64])
                .unwrap()
                .progress,
            RuntimeProgress::Pending
        );
        // begin_fence happened, but confirmation did not. Even an exact old
        // start retry must not execute while termination evidence is damaged.
        assert_eq!(
            runtime.start(TaskStart {
                scope: projection.scope(),
                generation: projection.generation(),
                definition: projection.definition(),
                execution_deadline: projection.execution_deadline(),
                input_schema: 1,
                input: &[9],
                checkpoint: None,
            }),
            Err(TaskPortError::Conflict)
        );
        std::fs::write(&manifest, saved).unwrap();
        runtime
            .fence(projection.scope(), projection.generation())
            .unwrap();
        runtime
            .fence(projection.scope(), projection.generation())
            .unwrap();
    }
}

#[test]
fn registry_symlink_is_not_followed_and_lock_timeout_does_not_confirm_termination() {
    let directory = tempfile::tempdir().unwrap();
    let definition = definition(2);
    let path = directory.path().join("tasks.db");
    let initial_journal =
        SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let mut configuration = config(directory.path());
    configuration.fence_timeout = Duration::from_millis(100);
    let initial = Runtime::new(authority, configuration).unwrap();
    let mut supervisor: Supervisor = Supervisor::restore(
        initial_journal,
        initial,
        scope(definition, &[9]),
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    supervisor.submit(&[9], 1, TaskTick::ZERO).unwrap();
    supervisor.advance(TaskTick::new(1)).unwrap();
    let projection = *supervisor.projection();
    let (mut journal, mut runtime) = supervisor.into_parts();
    let manifest = manifest(directory.path());
    let backup = directory.path().join("saved-manifest");
    std::fs::rename(&manifest, &backup).unwrap();
    symlink(&backup, &manifest).unwrap();
    assert_eq!(
        runtime.fence(projection.scope(), projection.generation()),
        Err(TaskPortError::Runtime)
    );
    std::fs::remove_file(&manifest).unwrap();
    std::fs::rename(&backup, &manifest).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.path().join("registry/registry.lock"))
        .unwrap();
    // Concurrent test launches can briefly inherit this descriptor between
    // fork and CLOEXEC. Wait for that setup boundary before holding the lock.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match flock(&lock, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => break,
            Err(error) => {
                assert_eq!(error, rustix::io::Errno::WOULDBLOCK);
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    assert_eq!(
        runtime.fence(projection.scope(), projection.generation()),
        Err(TaskPortError::Conflict)
    );
    drop(lock);
    // An independently committed successor must still be denied while the
    // predecessor's tombstone lacks confirmed termination.
    let mut successor = projection;
    let fault = successor
        .propose(
            TaskExecutionEvent::FaultObserved {
                fault: TaskFault::SupervisorInterrupted,
            },
            successor.generation(),
            TaskTick::new(2),
        )
        .unwrap();
    journal.commit(&fault, None).unwrap();
    successor.apply(&fault).unwrap();
    for tick in [3, 4] {
        let record = successor.next(TaskTick::new(tick)).unwrap().unwrap();
        journal.commit(&record, None).unwrap();
        successor.apply(&record).unwrap();
    }
    let request = TaskStart {
        scope: successor.scope(),
        generation: successor.generation(),
        definition,
        execution_deadline: successor.execution_deadline(),
        input_schema: 1,
        input: &[9],
        checkpoint: None,
    };
    assert_eq!(successor.generation(), TaskGeneration::new(2));
    assert_eq!(runtime.start(request), Err(TaskPortError::Conflict));
    runtime
        .fence(projection.scope(), projection.generation())
        .unwrap();
    runtime.start(request).unwrap();
    // Repeating an old fence must not stop the live successor.
    runtime
        .fence(projection.scope(), projection.generation())
        .unwrap();
    assert_eq!(
        runtime
            .poll(successor.scope(), successor.generation(), &mut [0; 64])
            .unwrap()
            .progress,
        RuntimeProgress::Pending
    );
    runtime
        .fence(successor.scope(), successor.generation())
        .unwrap();
    runtime
        .fence(projection.scope(), projection.generation())
        .unwrap();
}
