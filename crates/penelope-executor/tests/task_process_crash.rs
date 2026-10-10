//! Host process death at actual filesystem persistence boundaries. These are
//! test adapters, not a production durability certification (no power-loss claim).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_const_for_fn,
    clippy::panic,
    clippy::needless_pass_by_value,
    clippy::map_err_ignore
)]
#[path = "support/tasks.rs"]
mod support;
use penelope_core::task::runtime::*;
use penelope_core::task::*;
use penelope_executor::task::TaskSupervisor;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use support::{SumRuntime, definition, scope};

struct DiskJournal {
    root: PathBuf,
    pause: Option<&'static str>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskRecord {
    record: TaskRecord,
    // Test adapter protection against accidental corruption, not authentication.
    checksum: Digest,
}
impl DiskJournal {
    fn record_path(&self, sequence: u32) -> PathBuf {
        self.root.join(format!("record-{sequence}.json"))
    }
    fn blob_path(&self, digest: Digest) -> PathBuf {
        let hex: String = digest.0.iter().map(|byte| format!("{byte:02x}")).collect();
        self.root.join(format!("blob-{hex}"))
    }
    fn pause_at(&self, boundary: &str) {
        if self.pause == Some(boundary) {
            fs::write(self.root.join("ready-to-kill"), boundary).unwrap();
            loop {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    fn durable_write(&self, destination: PathBuf, bytes: &[u8]) -> Result<(), TaskPortError> {
        let temporary = destination.with_extension("pending");
        let mut file = fs::File::create(&temporary).map_err(|_| TaskPortError::Unavailable)?;
        file.write_all(bytes)
            .map_err(|_| TaskPortError::Unavailable)?;
        file.sync_all().map_err(|_| TaskPortError::Unavailable)?;
        fs::rename(temporary, destination).map_err(|_| TaskPortError::Unavailable)?;
        fs::File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| TaskPortError::Unavailable)?;
        Ok(())
    }
}
impl TaskJournal for DiskJournal {
    fn read(
        &mut self,
        scope: TaskScope,
        sequence: u32,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        match fs::read(self.record_path(sequence)) {
            Ok(bytes) => {
                let stored: DiskRecord =
                    serde_json::from_slice(&bytes).map_err(|_| TaskPortError::Corrupt)?;
                let record = stored.record;
                let encoded = serde_json::to_vec(&record).map_err(|_| TaskPortError::Corrupt)?;
                if Digest::of(&encoded) != stored.checksum {
                    return Err(TaskPortError::Corrupt);
                }
                if record.scope != scope {
                    return Err(TaskPortError::Conflict);
                }
                Ok(Some(record))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_error) => Err(TaskPortError::Unavailable),
        }
    }
    fn commit(
        &mut self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        if self.record_path(record.sequence).exists() {
            return Err(TaskPortError::Conflict);
        }
        if record.sequence > 0 && !self.record_path(record.sequence - 1).exists() {
            return Err(TaskPortError::Conflict);
        }
        self.pause_at("before-blob");
        if let Some(bytes) = blob {
            self.durable_write(self.blob_path(Digest::of(bytes)), bytes)?;
        }
        self.pause_at("after-blob");
        self.durable_write(
            self.record_path(record.sequence),
            &serde_json::to_vec(&DiskRecord {
                record: *record,
                checksum: Digest::of(
                    &serde_json::to_vec(record).map_err(|_| TaskPortError::Corrupt)?,
                ),
            })
            .map_err(|_| TaskPortError::Corrupt)?,
        )?;
        self.pause_at("after-record");
        Ok(*record)
    }
    fn read_blob(
        &mut self,
        _scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        let bytes = fs::read(self.blob_path(blob.digest)).map_err(|_| TaskPortError::Corrupt)?;
        buffer
            .get_mut(..bytes.len())
            .ok_or(TaskPortError::Exhausted)?
            .copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

#[test]
fn valid_json_corruption_is_rejected_by_journal_integrity_before_replay() {
    let root =
        std::env::temp_dir().join(format!("penelope-record-integrity-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let definition = definition(RecoveryMode::RestartFromCheckpoint, EffectContract::Pure);
    let scope = scope(definition, &[1; 64]);
    let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
        DiskJournal {
            root: root.clone(),
            pause: None,
        },
        SumRuntime::default(),
        scope,
        definition,
        0,
    )
    .unwrap();
    supervisor.submit(&[1; 64], 1, 0).unwrap();
    let (mut journal, _runtime) = supervisor.into_parts();
    let path = journal.record_path(0);
    let mut stored: DiskRecord = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    stored.record.tick = 9; // Still valid typed JSON and potentially plausible history.
    fs::write(path, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert_eq!(journal.read(scope, 0), Err(TaskPortError::Corrupt));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn crash_worker() {
    let Ok(root) = std::env::var("PENELOPE_TASK_CRASH_ROOT") else {
        return;
    };
    let boundary = match std::env::var("PENELOPE_TASK_CRASH_BOUNDARY")
        .unwrap()
        .as_str()
    {
        "before-blob" => "before-blob",
        "after-blob" => "after-blob",
        "after-record" => "after-record",
        "after-effect" => "after-effect",
        _ => panic!("bad boundary"),
    };
    let definition = definition(
        RecoveryMode::RestartFromCheckpoint,
        if boundary == "after-effect" {
            EffectContract::ReconcileRequired
        } else {
            EffectContract::Pure
        },
    );
    let scope = scope(definition, &[1; 64]);
    let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
        DiskJournal {
            root: root.into(),
            pause: None,
        },
        SumRuntime::default(),
        scope,
        definition,
        0,
    )
    .unwrap();
    supervisor.submit(&[1; 64], 1, 0).unwrap();
    supervisor.advance(1).unwrap();
    let (mut journal, runtime) = supervisor.into_parts();
    journal.pause = Some(boundary);
    if boundary == "after-effect" {
        journal
            .durable_write(journal.root.join("external-effect"), &1_u64.to_le_bytes())
            .unwrap();
        journal.pause_at("after-effect");
    }
    // Inject at the actual checkpoint persistence path while generation 1 is live.
    let projection = replay(
        scope,
        definition,
        [
            journal.read(scope, 0).unwrap().unwrap(),
            journal.read(scope, 1).unwrap().unwrap(),
        ],
    )
    .unwrap();
    let checkpoint = [8, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0];
    let record = projection
        .propose(
            TaskExecutionEvent::CheckpointCommitted {
                checkpoint: BlobRef {
                    digest: Digest::of(&checkpoint),
                    bytes: 12,
                    schema: 1,
                },
            },
            1,
            2,
        )
        .unwrap();
    let _runtime = runtime;
    journal.commit(&record, Some(&checkpoint)).unwrap();
    panic!("worker should have been killed")
}

#[test]
fn killed_process_recovers_only_committed_checkpoint() {
    for boundary in ["before-blob", "after-blob", "after-record", "after-effect"] {
        let root = std::env::temp_dir().join(format!(
            "penelope-task-crash-{}-{boundary}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_worker", "--nocapture"])
            .env("PENELOPE_TASK_CRASH_ROOT", &root)
            .env("PENELOPE_TASK_CRASH_BOUNDARY", boundary)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.join("ready-to-kill").exists() {
            if child.try_wait().unwrap().is_some() {
                panic!("worker exited before fault boundary");
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("worker failed to reach boundary");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        let definition = definition(
            RecoveryMode::RestartFromCheckpoint,
            if boundary == "after-effect" {
                EffectContract::ReconcileRequired
            } else {
                EffectContract::Pure
            },
        );
        let scope = scope(definition, &[1; 64]);
        let mut supervisor = TaskSupervisor::<_, _, 64>::restore(
            DiskJournal {
                root: root.clone(),
                pause: None,
            },
            SumRuntime::default(),
            scope,
            definition,
            3,
        )
        .unwrap();
        assert_eq!(supervisor.projection().status(), TaskStatus::Faulted);
        assert_eq!(
            supervisor.projection().checkpoint().is_some(),
            boundary == "after-record"
        );
        if boundary == "after-effect" {
            supervisor.advance(4).unwrap();
            assert_eq!(supervisor.projection().status(), TaskStatus::Reconciling);
            assert!(!supervisor.advance(5).unwrap());
            // Reconcile from actual independently persisted effect evidence.
            let evidence = fs::read(root.join("external-effect")).unwrap();
            assert_eq!(evidence, 1_u64.to_le_bytes());
            let output = BlobRef {
                digest: Digest::of(&evidence),
                bytes: 8,
                schema: 1,
            };
            supervisor
                .reconcile(
                    scope,
                    1,
                    Reconciliation::Completed {
                        output,
                        evidence: Digest::of(&evidence),
                    },
                    Some(&evidence),
                    6,
                )
                .unwrap();
            assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
            assert_eq!(supervisor.into_parts().1.starts, 0);
            assert_eq!(fs::read(root.join("external-effect")).unwrap(), evidence);
            fs::remove_dir_all(root).unwrap();
            continue;
        }
        for tick in 4..30 {
            if supervisor.projection().status() == TaskStatus::Completed {
                break;
            }
            if supervisor.projection().status() == TaskStatus::Running {
                supervisor.poll(tick).unwrap();
            } else {
                supervisor.advance(tick).unwrap();
            }
        }
        assert_eq!(supervisor.projection().status(), TaskStatus::Completed);
        let output = supervisor.projection().output().unwrap();
        let (mut journal, runtime) = supervisor.into_parts();
        assert_eq!(
            runtime.resumed,
            [if boundary == "after-record" { 8 } else { 0 }]
        );
        let mut bytes = [0; 64];
        let len = journal.read_blob(scope, output, &mut bytes).unwrap();
        assert_eq!(&bytes[..len], &64_u64.to_le_bytes());
        fs::remove_dir_all(root).unwrap();
    }
}
