//! Non-regular path regressions run in guarded children to contain blocking opens.
use super::*;
use std::{
    fs::OpenOptions,
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, fs::symlink},
    },
    process::Stdio,
};

fn fifo(path: &Path, writer: bool) -> Option<std::fs::File> {
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        path,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    writer.then(|| {
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .unwrap()
    })
}
fn image_case(case: &str) {
    let directory = tempfile::tempdir().unwrap();
    let definition = definition(2);
    let authority = SqliteTaskJournal::open(
        &directory.path().join("tasks.db"),
        definition,
        SqliteLimits::default(),
    )
    .unwrap();
    let path = directory.path().join("image");
    let mut configuration = config(directory.path());
    let mut source_handle = None;
    let writer = match case {
        "worker_fifo" => fifo(&path, false),
        "worker_fifo_open" => fifo(&path, true),
        "worker_directory" => {
            std::fs::create_dir(&path).unwrap();
            None
        }
        "worker_symlink" => {
            symlink(fixture(), &path).unwrap();
            None
        }
        "worker_procfd" => {
            let source = std::fs::File::open(fixture()).unwrap();
            configuration.worker = format!("/proc/self/fd/{}", source.as_raw_fd()).into();
            source_handle = Some(source);
            None
        }
        _ => panic!("unknown image case"),
    };
    if case != "worker_procfd" {
        configuration.worker = path.clone();
    }
    let result = Runtime::new(authority, configuration).map(|runtime| runtime.code_identity());
    drop(source_handle);
    drop(writer);
    if matches!(case, "worker_symlink" | "worker_procfd") {
        assert_eq!(result, Ok(definition.code));
    } else {
        assert_eq!(result, Err(TaskPortError::Corrupt));
        if case == "worker_directory" {
            std::fs::remove_dir(&path).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::copy(fixture(), &path).unwrap();
        let repair_authority = SqliteTaskJournal::open(
            &directory.path().join("tasks.db"),
            definition,
            SqliteLimits::default(),
        )
        .unwrap();
        let mut repaired = config(directory.path());
        repaired.worker = path;
        assert_eq!(
            Runtime::new(repair_authority, repaired)
                .unwrap()
                .code_identity(),
            definition.code
        );
    }
}
fn manifest_case(writer: bool) {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[9], 2);
    supervisor.advance(TaskTick::new(1)).unwrap();
    let projection = *supervisor.projection();
    let (_journal, mut runtime) = supervisor.into_parts();
    let path = std::fs::read_dir(directory.path().join("registry"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().is_some_and(|name| name.len() == 64))
        .unwrap();
    let saved = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let keeper = fifo(&path, writer);
    let rejected = runtime.fence(projection.scope(), projection.generation());
    let live = runtime
        .poll(projection.scope(), projection.generation(), &mut [0; 64])
        .map(|observation| observation.progress);
    let request = TaskStart {
        scope: projection.scope(),
        generation: projection.generation(),
        definition: projection.definition(),
        execution_deadline: projection.execution_deadline(),
        input_schema: 1,
        input: &[9],
        checkpoint: None,
    };
    let blocked = runtime.start(request);
    drop(keeper);
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, saved).unwrap();
    runtime
        .fence(projection.scope(), projection.generation())
        .unwrap();
    assert_eq!(rejected, Err(TaskPortError::Corrupt));
    assert_eq!(live, Ok(RuntimeProgress::Pending));
    assert_eq!(blocked, Err(TaskPortError::Conflict));
    assert_eq!(runtime.start(request), Err(TaskPortError::Conflict));
}
fn lock_case() {
    let directory = tempfile::tempdir().unwrap();
    let mut supervisor = fresh(directory.path(), &[9], 2);
    let path = directory.path().join("registry/registry.lock");
    assert!(!path.exists());
    let _writer = fifo(&path, false);
    let rejected = supervisor.advance(TaskTick::new(1));
    let launched = std::fs::read_dir(directory.path().join("registry"))
        .unwrap()
        .any(|entry| entry.unwrap().file_name().len() == 64);
    let projection = *supervisor.projection();
    let (_journal, mut runtime) = supervisor.into_parts();
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, b"").unwrap();
    runtime
        .fence(projection.scope(), projection.generation())
        .unwrap();
    assert_eq!(rejected, Err(SupervisorError::Port(TaskPortError::Corrupt)));
    assert!(!launched);
}
#[test]
fn filesystem_case_helper() {
    let Ok(case) = std::env::var("PENELOPE_FILESYSTEM_CASE") else {
        return;
    };
    match case.as_str() {
        "worker_fifo" | "worker_fifo_open" | "worker_directory" | "worker_symlink"
        | "worker_procfd" => image_case(&case),
        "manifest_fifo" => manifest_case(false),
        "manifest_fifo_open" => manifest_case(true),
        "lock_fifo" => lock_case(),
        _ => panic!("unknown filesystem fixture"),
    }
}
fn guarded(case: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "filesystem_paths::filesystem_case_helper",
            "--nocapture",
        ])
        .env("PENELOPE_FILESYSTEM_CASE", case)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now().checked_add(Duration::from_secs(5)).unwrap();
    let mut timed_out = false;
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!timed_out, "{case}: filesystem operation blocked");
    assert!(
        output.status.success(),
        "{case}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn worker_fifo_without_writer_is_rejected() {
    guarded("worker_fifo");
}
#[test]
fn worker_fifo_with_writer_is_rejected() {
    guarded("worker_fifo_open");
}
#[test]
fn worker_directory_is_rejected() {
    guarded("worker_directory");
}
#[test]
fn worker_symlink_to_regular_image_remains_valid() {
    guarded("worker_symlink");
}
#[test]
fn worker_procfd_regular_image_remains_valid() {
    guarded("worker_procfd");
}
#[test]
fn manifest_fifo_without_writer_is_rejected_and_repairable() {
    guarded("manifest_fifo");
}
#[test]
fn manifest_fifo_with_writer_is_rejected_and_repairable() {
    guarded("manifest_fifo_open");
}
#[test]
fn registry_lock_fifo_is_rejected_and_repairable() {
    guarded("lock_fifo");
}
