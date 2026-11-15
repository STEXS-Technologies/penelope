//! SIGKILL between independently durable source and receiver acknowledgements.
use super::*;
use std::{
    os::unix::process::ExitStatusExt,
    process::Command,
    time::{Duration, Instant},
};
fn cut(phase: &str) {
    if std::env::var("PENELOPE_TASK_HANDOFF_CUT").ok().as_deref() != Some(phase) {
        return;
    }
    std::fs::write(
        std::env::var_os("PENELOPE_TASK_HANDOFF_MARKER").unwrap(),
        phase,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("parent did not kill native handoff child");
}
struct CutJournal(Arc<SqliteAsyncTaskJournal>);
#[async_trait]
impl AsyncTaskJournal for CutJournal {
    async fn read(
        &self,
        scope: TaskScope,
        sequence: TaskSequence,
    ) -> Result<Option<TaskRecord>, TaskPortError> {
        self.0.read(scope, sequence).await
    }
    async fn commit(
        &self,
        record: &TaskRecord,
        blob: Option<&[u8]>,
    ) -> Result<TaskRecord, TaskPortError> {
        cut("before-native-commit");
        let accepted = self.0.commit(record, blob).await?;
        cut("after-native-commit");
        Ok(accepted)
    }
    async fn read_blob(
        &self,
        scope: TaskScope,
        blob: BlobRef,
        buffer: &mut [u8],
    ) -> Result<usize, TaskPortError> {
        self.0.read_blob(scope, blob, buffer).await
    }
}
#[test]
fn handoff_process_helper() {
    let Some(path) = std::env::var_os("PENELOPE_TASK_HANDOFF_MACHINE") else {
        return;
    };
    let store = Arc::new(machine::open(std::path::Path::new(&path)));
    let tasks = Arc::new(CutJournal(journal(std::path::Path::new(
        &std::env::var_os("PENELOPE_TASK_HANDOFF_TASK").unwrap(),
    ))));
    let receiver = Arc::new(MachineTaskReceiver::new(tasks, NAMESPACE, definition(), 1).unwrap());
    let outbox = SqliteMachineOutbox::new(
        store,
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        OUTBOX,
    )
    .unwrap();
    let source = block_on(outbox.status(&true, SCOPE, MachineIntentId::from_bytes([4; 32])))
        .unwrap()
        .unwrap()
        .source;
    let lease = MachineIntentLease::decode(
        MachineIntentLeaseMetadata {
            source,
            owner: claim().owner,
            token: MachineLeaseToken::new(1),
            expires: MachineLeaseTick::new(110),
            attempt: MachineDeliveryAttempt::new(1),
        },
        &13_u64.to_le_bytes(),
        8,
    )
    .unwrap();
    cut("before-source-validation");
    let accepted = block_on(receiver.accept(&outbox, &true, &lease, TaskTick::new(1))).unwrap();
    cut("before-source-ack");
    block_on(outbox.acknowledge(&true, &lease, &accepted)).unwrap();
    cut("after-source-ack");
    panic!("handoff cut not reached");
}
#[test]
fn sigkill_handoff_reconstructs_native_intent_and_never_guesses_source_ack() {
    for shared in [false, true] {
        for phase in [
            "before-source-validation",
            "before-native-commit",
            "after-native-commit",
            "before-source-ack",
            "after-source-ack",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let machine_path = directory.path().join("machine.db");
            let task_path = if shared {
                machine_path.clone()
            } else {
                directory.path().join("tasks.db")
            };
            let store = source(&machine_path);
            let receiver = Arc::new(
                MachineTaskReceiver::new(journal(&task_path), NAMESPACE, definition(), 1).unwrap(),
            );
            let outbox = SqliteMachineOutbox::new(
                store,
                Arc::new(Clock(AtomicU64::new(100))),
                receiver,
                OUTBOX,
            )
            .unwrap();
            let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
            drop(outbox);
            let marker = directory.path().join("marker");
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "crash::handoff_process_helper", "--nocapture"])
                .env("PENELOPE_TASK_HANDOFF_MACHINE", &machine_path)
                .env("PENELOPE_TASK_HANDOFF_TASK", &task_path)
                .env("PENELOPE_TASK_HANDOFF_CUT", phase)
                .env("PENELOPE_TASK_HANDOFF_MARKER", &marker)
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while std::fs::read(&marker).ok().as_deref() != Some(phase.as_bytes()) {
                if Instant::now() >= deadline {
                    drop(child.kill());
                    drop(child.wait());
                    panic!("handoff failed to reach {phase}");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            child.kill().unwrap();
            assert_eq!(child.wait().unwrap().signal(), Some(9));
            let reopened_source = Arc::new(machine::open(&machine_path));
            let reopened_tasks = journal(&task_path);
            let reopened_receiver = Arc::new(
                MachineTaskReceiver::new(Arc::clone(&reopened_tasks), NAMESPACE, definition(), 1)
                    .unwrap(),
            );
            let reopened_outbox = SqliteMachineOutbox::new(
                reopened_source,
                Arc::new(Clock(AtomicU64::new(100))),
                Arc::clone(&reopened_receiver),
                OUTBOX,
            )
            .unwrap();
            let binding = reopened_receiver
                .binding(lease.metadata().source, lease.payload())
                .unwrap();
            let found = block_on(AsyncTaskJournal::read(
                &*reopened_tasks,
                binding.scope(),
                TaskSequence::ZERO,
            ))
            .unwrap();
            let native_committed = matches!(
                phase,
                "after-native-commit" | "before-source-ack" | "after-source-ack"
            );
            assert_eq!(found.is_some(), native_committed, "{phase} shared={shared}");
            let status = block_on(reopened_outbox.status(
                &true,
                SCOPE,
                lease.metadata().source.intent.identity,
            ))
            .unwrap()
            .unwrap();
            assert_eq!(
                status.accepted.is_some(),
                phase == "after-source-ack",
                "{phase}"
            );
            let accepted = if phase == "after-source-ack" {
                found.unwrap()
            } else {
                block_on(reopened_receiver.accept(
                    &reopened_outbox,
                    &true,
                    &lease,
                    TaskTick::new(2),
                ))
                .unwrap()
            };
            assert_eq!(
                accepted.tick,
                TaskTick::new(if native_committed { 1 } else { 2 })
            );
            block_on(reopened_outbox.acknowledge(&true, &lease, &accepted)).unwrap();
            assert_eq!(
                block_on(AsyncTaskJournal::read(
                    &*reopened_tasks,
                    binding.scope(),
                    TaskSequence::new(1)
                ))
                .unwrap(),
                None
            );
        }
    }
}
