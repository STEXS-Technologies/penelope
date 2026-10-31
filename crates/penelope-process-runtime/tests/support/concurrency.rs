//! Independent supervisors share actual SQLite authorization and the Linux registry.
use super::*;

const INPUT: [u8; 4] = [0, 1, 2, 3];

fn marker(path: &Path, expected: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while std::fs::read(path).ok().as_deref() != Some(expected) {
        assert!(Instant::now() < deadline, "supervisor barrier timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn competing_supervisor_helper() {
    let Some(directory) = std::env::var_os("PENELOPE_COMPETING_SUPERVISOR_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let identity = std::env::var("PENELOPE_COMPETING_SUPERVISOR_ID").unwrap();
    let definition = definition(2);
    let path = directory.join("tasks.db");
    let journal = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let authority = SqliteTaskJournal::open(&path, definition, SqliteLimits::default()).unwrap();
    let runtime = Runtime::new(authority, config(&directory)).unwrap();
    let mut supervisor = Supervisor::restore(
        journal,
        runtime,
        scope(definition, &INPUT),
        definition,
        TaskTick::ZERO,
    )
    .unwrap();
    assert_eq!(supervisor.projection().status(), TaskStatus::Pending);
    std::fs::write(directory.join(format!("ready-{identity}")), b"ready").unwrap();
    marker(&directory.join("gate"), b"go");
    let result = match supervisor.advance(TaskTick::new(1)) {
        Ok(true) => {
            assert_eq!(supervisor.projection().status(), TaskStatus::Running);
            assert_eq!(wait(&mut supervisor), Ok(true));
            assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
            b"completed".as_slice()
        }
        Err(SupervisorError::Port(TaskPortError::Conflict)) => {
            assert_eq!(
                supervisor.advance(TaskTick::new(2)),
                Err(SupervisorError::ReconstructionRequired)
            );
            b"conflict".as_slice()
        }
        result => panic!("unexpected competing supervisor result: {result:?}"),
    };
    std::fs::write(directory.join(format!("result-{identity}")), result).unwrap();
}

#[test]
fn two_supervisor_processes_admit_one_real_worker_and_replay_one_completion() {
    let directory = tempfile::tempdir().unwrap();
    // Both child supervisors must restore this same committed intent before
    // either is allowed to attempt Started(1). No parent runtime starts a worker.
    drop(fresh(directory.path(), &INPUT, 2));
    let mut children = Vec::new();
    for identity in ["a", "b"] {
        children.push(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "concurrency::competing_supervisor_helper",
                    "--nocapture",
                ])
                .env("PENELOPE_COMPETING_SUPERVISOR_DIR", directory.path())
                .env("PENELOPE_COMPETING_SUPERVISOR_ID", identity)
                .spawn()
                .unwrap(),
        );
    }
    for identity in ["a", "b"] {
        marker(
            &directory.path().join(format!("ready-{identity}")),
            b"ready",
        );
    }
    std::fs::write(directory.path().join("gate"), b"go").unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    for child in &mut children {
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() >= deadline {
                for pending in &mut children {
                    drop(pending.kill());
                    drop(pending.wait());
                }
                panic!("competing supervisors failed to finish");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let mut results: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|identity| std::fs::read(directory.path().join(format!("result-{identity}"))).unwrap())
        .collect();
    results.sort();
    assert_eq!(results, [b"completed".to_vec(), b"conflict".to_vec()]);
    let definition = definition(2);
    let scope = scope(definition, &INPUT);
    let mut journal = SqliteTaskJournal::open(
        &directory.path().join("tasks.db"),
        definition,
        SqliteLimits::default(),
    )
    .unwrap();
    let mut projection = TaskProjection::new(scope, definition).unwrap();
    for sequence in 0..3 {
        let record = journal
            .read(scope, TaskSequence::new(sequence))
            .unwrap()
            .unwrap();
        projection.apply(&record).unwrap();
    }
    assert_eq!(journal.read(scope, TaskSequence::new(3)).unwrap(), None);
    assert_eq!(projection.status(), TaskStatus::Completed);
    assert_eq!(projection.generation(), TaskGeneration::new(1));
    let mut output = [0; 8];
    journal
        .read_blob(scope, projection.output().unwrap(), &mut output)
        .unwrap();
    assert_eq!(u64::from_le_bytes(output), 6);
    let authority = SqliteTaskJournal::open(
        &directory.path().join("tasks.db"),
        definition,
        SqliteLimits::default(),
    )
    .unwrap();
    let restored = Supervisor::restore(
        journal,
        Runtime::new(authority, config(directory.path())).unwrap(),
        scope,
        definition,
        TaskTick::new(3),
    )
    .unwrap();
    assert_eq!(*restored.projection(), projection);
}
