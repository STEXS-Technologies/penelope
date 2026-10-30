//! Irreversible setup is exercised only in fresh subprocesses, never the runner.
#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;
use std::{
    fs::OpenOptions,
    os::unix::fs::FileExt,
    process::Stdio,
    time::{Duration, Instant},
};

#[test]
fn isolated_child_setup_helper() {
    let Ok(mode) = std::env::var("PENELOPE_CHILD_SETUP_MODE") else {
        return;
    };
    let path = std::env::var_os("PENELOPE_CHILD_SETUP_MANIFEST").unwrap();
    let manifest = OpenOptions::new()
        .read(true)
        .write(mode != "readonly")
        .open(path)
        .unwrap();
    // std opens CLOEXEC by default; clear it so the success assertion actually
    // proves close_range marked an inherited descriptor.
    rustix::io::fcntl_setfd(&manifest, rustix::io::FdFlags::empty()).unwrap();
    assert!(
        !rustix::io::fcntl_getfd(&manifest)
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
    let before_limits =
        [Resource::Cpu, Resource::As, Resource::Core].map(rustix::process::getrlimit);
    let parent = if mode == "wrong-parent" {
        getpid()
    } else {
        getppid().unwrap()
    };
    let file_limit = rustix::process::getrlimit(Resource::Fsize);
    if mode == "short-write" {
        setrlimit(
            Resource::Fsize,
            Rlimit {
                current: Some(165),
                ..file_limit
            },
        )
        .unwrap();
    }
    let result = prepare_child(parent, &manifest, 30, 1024 * 1024 * 1024);
    if mode == "short-write" {
        setrlimit(Resource::Fsize, file_limit).unwrap();
    }
    if mode != "success" {
        assert_eq!(
            [Resource::Cpu, Resource::As, Resource::Core].map(rustix::process::getrlimit),
            before_limits
        );
        assert!(
            !rustix::io::fcntl_getfd(&manifest)
                .unwrap()
                .contains(rustix::io::FdFlags::CLOEXEC)
        );
    }
    match mode.as_str() {
        "wrong-parent" => {
            assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::ECHILD));
            let mut bytes = [0; 12];
            manifest.read_exact_at(&mut bytes, IDENTITY_OFFSET).unwrap();
            assert_eq!(bytes, [0; 12]);
        }
        "readonly" => {
            assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EPERM));
            let mut bytes = [0; 12];
            manifest.read_exact_at(&mut bytes, IDENTITY_OFFSET).unwrap();
            assert_eq!(bytes, [0; 12]);
        }
        "short-write" => {
            assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EIO));
            let mut bytes = [0; 12];
            manifest.read_exact_at(&mut bytes, IDENTITY_OFFSET).unwrap();
            assert_eq!(
                bytes.get(..4).unwrap(),
                u32::try_from(getpid().as_raw_nonzero().get())
                    .unwrap()
                    .to_be_bytes()
            );
            assert_eq!(bytes.get(5..).unwrap(), [0; 7]);
        }
        "success" => {
            result.unwrap();
            let identity = self_identity().unwrap();
            let mut bytes = [0; 12];
            manifest.read_exact_at(&mut bytes, IDENTITY_OFFSET).unwrap();
            assert_eq!(bytes.get(..4).unwrap(), identity.pid.to_be_bytes());
            assert_eq!(bytes.get(4..).unwrap(), identity.start_ticks.to_be_bytes());
            for (resource, limit) in [
                (Resource::Cpu, 30),
                (Resource::As, 1024 * 1024 * 1024),
                (Resource::Core, 0),
            ] {
                assert_eq!(
                    rustix::process::getrlimit(resource),
                    Rlimit {
                        current: Some(limit),
                        maximum: Some(limit),
                    }
                );
            }
            assert!(
                rustix::io::fcntl_getfd(&manifest)
                    .unwrap()
                    .contains(rustix::io::FdFlags::CLOEXEC)
            );
            assert_eq!(
                rustix::process::parent_process_death_signal().unwrap(),
                Some(Signal::KILL)
            );
        }
        _ => panic!("unknown child setup mode"),
    }
}

#[test]
fn isolated_setup_persists_kernel_identity_and_bounds_and_rejects_invalid_parent_or_file() {
    for mode in ["success", "wrong-parent", "readonly", "short-write"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manifest");
        std::fs::write(&path, [0; 172]).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sandbox::tests::isolated_child_setup_helper",
                "--nocapture",
            ])
            .env("PENELOPE_CHILD_SETUP_MODE", mode)
            .env("PENELOPE_CHILD_SETUP_MANIFEST", path)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "child setup {mode} failed: {status}");
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("child setup {mode} timed out");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[test]
fn invalid_spawn_budgets_do_not_execute_a_command_or_mutate_manifest() {
    for (cpu, memory) in [(0, 1024 * 1024 * 1024), (1, 8 * 1024 * 1024 - 1)] {
        let file = tempfile::tempfile().unwrap();
        let observer = file.try_clone().unwrap();
        let mut command = Command::new("/nonexistent/penelope-worker");
        assert!(matches!(
            spawn(&mut command, filter().unwrap(), file, cpu, memory),
            Err(TaskPortError::Exhausted)
        ));
        assert_eq!(observer.metadata().unwrap().len(), 0);
    }
}
