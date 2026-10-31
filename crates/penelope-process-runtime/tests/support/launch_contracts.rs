//! Admission failures never authorize an execution; retries preserve identity.
use super::*;

#[test]
fn live_capacity_does_not_reject_the_exact_reserved_start_retry() {
    let directory = tempfile::tempdir().unwrap();
    let definition = definition(2);
    let input = [9]; // Sleeping worker remains alive until explicitly fenced.
    let scope = scope(definition, &input);
    let path = directory.path().join("tasks.db");
    let initial_journal =
        SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let mut configuration = config(directory.path());
    configuration.max_live = 1;
    let initial_runtime = Runtime::new(authority, configuration).unwrap();
    let mut supervisor: Supervisor = TaskSupervisor::restore(
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
    let (mut journal, mut runtime) = supervisor.into_parts();
    let request = TaskStart {
        scope,
        generation: projection.generation(),
        definition,
        execution_deadline: projection.execution_deadline(),
        input_schema: 1,
        input: &input,
        checkpoint: None,
    };
    let manifest = std::fs::read_dir(directory.path().join("registry"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|manifest_path| {
            manifest_path
                .file_name()
                .is_some_and(|name| name.len() == 64)
        })
        .unwrap();
    let identity = std::fs::read(&manifest).unwrap();
    for _ in 0..3 {
        runtime.start(request).unwrap();
    }
    assert_eq!(std::fs::read(manifest).unwrap(), identity);
    let foreign = TaskScope {
        task: TaskId::from_digest(Digest([8; 32])),
        ..scope
    };
    assert_eq!(
        runtime.start(TaskStart {
            scope: foreign,
            ..request
        }),
        Err(TaskPortError::Exhausted)
    );
    assert_eq!(journal.read(foreign, TaskSequence::ZERO).unwrap(), None);
    // Capacity admission never bypasses the authority's full input/schema check.
    assert!(
        runtime
            .start(TaskStart {
                input_schema: 2,
                ..request
            })
            .is_err()
    );
    runtime.fence(scope, projection.generation()).unwrap();
    assert_eq!(runtime.start(request), Err(TaskPortError::Conflict));
}

#[test]
fn invalid_runtime_limits_are_rejected_before_any_worker_can_start() {
    let directory = tempfile::tempdir().unwrap();
    let definition = definition(2);
    let path = directory.path().join("tasks.db");
    for case in 0..11 {
        let authority =
            SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
        let mut configuration = config(directory.path());
        match case {
            0 => configuration.max_live = 0,
            1 => configuration.max_live = 257,
            2 => configuration.max_frame_bytes = 0,
            3 => configuration.max_frame_bytes = 16 * 1024 * 1024 + 1,
            4 => configuration.ticks_per_second = 0,
            5 => configuration.address_space_bytes = 8 * 1024 * 1024 - 1,
            6 => configuration.startup_timeout = Duration::ZERO,
            7 => configuration.startup_timeout = Duration::from_secs(31),
            8 => configuration.fence_timeout = Duration::ZERO,
            9 => configuration.fence_timeout = Duration::from_secs(31),
            _ => configuration.max_worker_bytes = 0,
        }
        assert_eq!(
            Runtime::new(authority, configuration).err(),
            Some(TaskPortError::Exhausted)
        );
    }
    let mut journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    assert_eq!(
        journal
            .read(scope(definition, b"input"), TaskSequence::ZERO)
            .unwrap(),
        None
    );
}

#[test]
fn wrong_architecture_truncated_and_dynamic_images_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let definition = definition(2);
    let path = directory.path().join("tasks.db");
    let original = std::fs::read(fixture()).unwrap();
    let worker = directory.path().join("modified-worker");
    for case in 0..7 {
        let mut image = original.clone();
        match case {
            0 => image.truncate(12),
            1 => image
                .get_mut(18..20)
                .unwrap()
                .copy_from_slice(&183_u16.to_le_bytes()),
            2 => image
                .get_mut(16..18)
                .unwrap()
                .copy_from_slice(&1_u16.to_le_bytes()),
            3 => image
                .get_mut(54..56)
                .unwrap()
                .copy_from_slice(&0_u16.to_le_bytes()),
            4 => image
                .get_mut(56..58)
                .unwrap()
                .copy_from_slice(&129_u16.to_le_bytes()),
            5 => image
                .get_mut(32..40)
                .unwrap()
                .copy_from_slice(&u64::MAX.to_le_bytes()),
            _ => {
                let offset = usize::try_from(u64::from_le_bytes(
                    image.get(32..40).unwrap().try_into().unwrap(),
                ))
                .unwrap();
                // A PT_INTERP header requests a dynamic loader, outside this profile.
                image
                    .get_mut(offset..offset + 4)
                    .unwrap()
                    .copy_from_slice(&3_u32.to_le_bytes());
            }
        }
        std::fs::write(&worker, image).unwrap();
        let authority =
            SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
        let mut configuration = config(directory.path());
        configuration.worker = worker.clone();
        let expected = if case == 6 {
            TaskPortError::Runtime
        } else {
            TaskPortError::Corrupt
        };
        assert_eq!(Runtime::new(authority, configuration).err(), Some(expected));
    }
}

#[test]
fn sealed_worker_survives_replacement_of_its_source_file() {
    let directory = tempfile::tempdir().unwrap();
    let definition = definition(2);
    let input = [0, 1, 2, 3];
    let scope = scope(definition, &input);
    let path = directory.path().join("tasks.db");
    let worker = directory.path().join("worker");
    std::fs::copy(fixture(), &worker).unwrap();
    let initial_journal =
        SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let mut configuration = config(directory.path());
    configuration.worker = worker.clone();
    let initial_runtime = Runtime::new(authority, configuration).unwrap();
    assert_eq!(initial_runtime.code_identity(), definition.code);
    std::fs::write(worker, b"replaced source is not executable ELF").unwrap();
    let mut supervisor: Supervisor = TaskSupervisor::restore(
        initial_journal,
        initial_runtime,
        scope,
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    supervisor.submit(&input, 1, TaskTick::ZERO).unwrap();
    supervisor.advance(TaskTick::new(1)).unwrap();
    wait(&mut supervisor).unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
    let output = supervisor.projection().output().unwrap();
    let (mut journal, _runtime) = supervisor.into_parts();
    let mut bytes = [0; 8];
    journal.read_blob(scope, output, &mut bytes).unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 6);
}

#[test]
fn expired_startup_write_poisoning_requires_reconstruction_and_fences_worker() {
    let directory = tempfile::tempdir().unwrap();
    let definition = definition(2);
    let input = [9];
    let scope = scope(definition, &input);
    let path = directory.path().join("tasks.db");
    let journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let mut configuration = config(directory.path());
    configuration.startup_timeout = Duration::from_nanos(1);
    let runtime = Runtime::new(authority, configuration).unwrap();
    let mut supervisor: Supervisor =
        TaskSupervisor::restore(journal, runtime, scope, definition, TaskTick::ZERO).unwrap();
    supervisor.submit(&input, 1, TaskTick::ZERO).unwrap();
    assert_eq!(
        supervisor.advance(TaskTick::new(1)),
        Err(TaskPortError::Runtime.into())
    );
    assert_eq!(supervisor.projection().status(), TaskStatus::Running);
    assert_eq!(
        supervisor.poll(TaskTick::new(2)),
        Err(SupervisorError::ReconstructionRequired)
    );
    let (retained_journal, mut failed_runtime) = supervisor.into_parts();
    let restored_authority =
        SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let restored_runtime = Runtime::new(restored_authority, config(directory.path())).unwrap();
    let recovered = Supervisor::restore(
        retained_journal,
        restored_runtime,
        scope,
        definition,
        TaskTick::new(2),
    )
    .unwrap();
    assert_eq!(recovered.projection().status(), TaskStatus::Faulted);
    assert_eq!(recovered.projection().output(), None);
    let generation = recovered.projection().generation();
    // The reconstructed runtime proves termination; the original child handle
    // can now reap without blocking or changing the recovered history.
    failed_runtime.fence(scope, generation).unwrap();
    let (mut recovered_journal, mut recovered_runtime) = recovered.into_parts();
    assert_eq!(
        recovered_journal
            .read(scope, TaskSequence::new(2))
            .unwrap()
            .unwrap()
            .event,
        TaskExecutionEvent::FaultObserved {
            fault: TaskFault::SupervisorInterrupted,
        }
    );
    assert_eq!(
        recovered_runtime.start(TaskStart {
            scope,
            generation,
            definition,
            execution_deadline: TaskTick::new(51),
            input_schema: 1,
            input: &input,
            checkpoint: None,
        }),
        Err(TaskPortError::Conflict)
    );
}
