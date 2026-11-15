//! Real SIGKILL across independently durable machine/saga acknowledgements.
use super::*;
use std::{
    os::unix::process::ExitStatusExt,
    process::Command,
    time::{Duration, Instant},
};
pub(super) fn cut(phase: &str) {
    if std::env::var("PENELOPE_SAGA_HANDOFF_CUT").ok().as_deref() != Some(phase) {
        return;
    }
    std::fs::write(
        std::env::var_os("PENELOPE_SAGA_HANDOFF_MARKER").unwrap(),
        phase,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("parent did not kill handoff child");
}
#[test]
fn saga_handoff_process_helper() {
    let Some(root) = std::env::var_os("PENELOPE_SAGA_HANDOFF_ROOT") else {
        return;
    };
    let root = std::path::Path::new(&root);
    let shared = std::env::var("PENELOPE_SAGA_SHARED").unwrap() == "true";
    let graph = std::env::var("PENELOPE_SAGA_GRAPH").unwrap() == "true";
    let native_path = if shared {
        root.join("source.db")
    } else {
        root.join("native.db")
    };
    let native = Arc::new(NativeFaults {
        inner: process(&native_path),
        lose_ack: AtomicBool::new(false),
        barrier: None,
    });
    let receiver = make_receiver(native, Policy::new(true), graph);
    let store = Arc::new(machine::open(&root.join("source.db")));
    let outbox = SqliteMachineOutbox::new(
        store,
        Arc::new(Clock(AtomicU64::new(100))),
        Arc::clone(&receiver),
        LIMITS,
    )
    .unwrap();
    let lease = block_on(outbox.claim(&true, claim())).unwrap().remove(0);
    cut("before-source-validation");
    let receipt = block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(1))).unwrap();
    cut("before-source-ack");
    block_on(outbox.acknowledge(&true, &lease, &receipt)).unwrap();
    cut("after-source-ack");
    panic!("cut not reached");
}
#[test]
fn sigkill_reconstructs_exact_native_saga_and_separate_source_acceptance() {
    for graph in [false, true] {
        for shared in [false, true] {
            for phase in [
                "before-source-validation",
                "before-native-commit",
                "after-native-commit",
                "before-source-ack",
                "after-source-ack",
            ] {
                let directory = tempfile::tempdir().unwrap();
                let root = directory.path();
                let source_path = root.join("source.db");
                let native_path = if shared {
                    source_path.clone()
                } else {
                    root.join("native.db")
                };
                drop(source(&source_path, 8));
                drop(process(&native_path));
                let marker = root.join("marker");
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "crash::saga_handoff_process_helper",
                        "--nocapture",
                    ])
                    .env("PENELOPE_SAGA_HANDOFF_ROOT", root)
                    .env("PENELOPE_SAGA_SHARED", shared.to_string())
                    .env("PENELOPE_SAGA_GRAPH", graph.to_string())
                    .env("PENELOPE_SAGA_HANDOFF_CUT", phase)
                    .env("PENELOPE_SAGA_HANDOFF_MARKER", &marker)
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while std::fs::read(&marker).ok().as_deref() != Some(phase.as_bytes()) {
                    if Instant::now() >= deadline {
                        drop(child.kill());
                        drop(child.wait());
                        panic!("child did not reach {phase}");
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                child.kill().unwrap();
                assert_eq!(child.wait().unwrap().signal(), Some(9));
                let store = Arc::new(machine::open(&source_path));
                let native = process(&native_path);
                let receiver = make_receiver(Arc::clone(&native), Policy::new(true), graph);
                let outbox = SqliteMachineOutbox::new(
                    Arc::clone(&store),
                    Arc::new(Clock(AtomicU64::new(110))),
                    Arc::clone(&receiver),
                    LIMITS,
                )
                .unwrap();
                let commit =
                    block_on(store.lookup(&true, SCOPE, MachineEventId::from_bytes([4; 32])))
                        .unwrap()
                        .unwrap();
                let intent = commit.batch().intents().next().unwrap().0.identity;
                let status = block_on(outbox.status(&true, SCOPE, intent))
                    .unwrap()
                    .unwrap();
                assert_eq!(status.accepted.is_some(), phase == "after-source-ack");
                let present = !matches!(phase, "before-source-validation" | "before-native-commit");
                let sql = rusqlite::Connection::open(&native_path).unwrap();
                let count: i64 = sql
                    .query_row("SELECT count(*) FROM process_outcomes", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(count, if present { 3 } else { 0 });
                let claims = block_on(outbox.claim(&true, claim())).unwrap();
                if phase == "after-source-ack" {
                    assert!(claims.is_empty());
                } else {
                    let lease = claims.into_iter().next().unwrap();
                    assert!(lease.requires_reconciliation());
                    let receipt =
                        block_on(receiver.accept(&outbox, &true, &lease, LogicalTime(2))).unwrap();
                    assert_eq!(
                        receipt.commit().outcomes[0].occurred_at,
                        LogicalTime(if present { 1 } else { 2 })
                    );
                    block_on(outbox.acknowledge(&true, &lease, &receipt)).unwrap();
                }
                let final_outcomes: i64 = sql
                    .query_row("SELECT count(*) FROM process_outcomes", [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(final_outcomes, 3);
                let final_actions: i64 = sql
                    .query_row("SELECT count(*) FROM process_actions", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(final_actions, 1);
            }
        }
    }
}
