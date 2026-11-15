//! Identical public-port producer/reader compiled against the old and new adapters.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use penelope_core::task::{runtime::*, *};
use penelope_domain::*;
use penelope_ports::{payloads::ProcessPayloadStore, *};
use penelope_sqlite::*;
use std::{
    future::Future,
    num::NonZeroU16,
    path::Path,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
const fn definition() -> TaskDefinition {
    TaskDefinition {
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
            max_input_bytes: 64,
            max_output_bytes: 64,
            max_history_records: 32,
            max_checkpoint_commits: 8,
            cpu_ticks: 10,
            deadline_tick: TaskTick::new(10000),
        },
    }
}
fn scope() -> TaskScope {
    TaskScope {
        tenant: TaskTenantId::from_digest(Digest([1; 32])),
        task: TaskId::from_digest(Digest([2; 32])),
        definition: definition().fingerprint(),
        input: TaskInputDigest::from_digest(Digest::of(b"input")),
    }
}
fn intent() -> TaskRecord {
    TaskProjection::new(scope(), definition())
        .unwrap()
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition: definition(),
                input: BlobRef {
                    digest: Digest::of(b"input"),
                    bytes: 5,
                    schema: 1,
                },
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        )
        .unwrap()
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(Instant::now() < deadline, "async operation timed out");
        std::thread::park_timeout(Duration::from_millis(10));
    }
}
struct TestClock {
    now: AtomicU64,
    reactor: std::thread::ThreadId,
}
impl SqliteLeaseClock for TestClock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        if std::thread::current().id() == self.reactor {
            return Err(PortError::Invariant);
        }
        Ok(LogicalTime(self.now.load(Ordering::SeqCst)))
    }
}
fn clock() -> Arc<TestClock> {
    Arc::new(TestClock {
        now: AtomicU64::new(100),
        reactor: std::thread::current().id(),
    })
}
fn id<T: FromStr>(text: &str) -> T {
    text.parse().ok().unwrap()
}
fn process_scope() -> ProcessScope {
    ProcessScope::new(
        id("tnt_sqlite"),
        id("prc_sqlite"),
        id("def_sqlite"),
        id("dfv_one"),
        ContentDigest([9; 32]),
    )
}
fn commit(timer: bool) -> AtomicProcessCommit {
    let scope = process_scope();
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        id("inp_start"),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([1; 32]),
    );
    let outcome = ProcessOutcome::new(
        scope.clone(),
        0,
        ProcessOutcomeFact::new(
            id("out_start"),
            CausationId::Input(input.input_id.clone()),
            OutcomeActor::System,
            LogicalTime(1),
            ProcessOutcomeKind::Started,
            ContentDigest([2; 32]),
        ),
    );
    let action = ProcessAction::new(
        scope,
        id("act_work"),
        id("stp_work"),
        0,
        if timer {
            ProcessActionKind::Timer
        } else {
            ProcessActionKind::ExternalEffect
        },
        ContentDigest([3; 32]),
    );
    AtomicProcessCommit::new(0, Some(input), vec![outcome], vec![action]).unwrap()
}
fn store(path: &std::path::Path, clock: Arc<TestClock>) -> SqliteProcessStore {
    SqliteProcessStore::open(path, SqliteLimits::default(), clock, 10, 16).unwrap()
}

fn inspect(path: &Path) {
    let mut journal = SqliteTaskJournal::open(path, definition(), SqliteLimits::default()).unwrap();
    let mut projection = TaskProjection::new(scope(), definition()).unwrap();
    for sequence in 0..2 {
        let record = journal
            .read(scope(), TaskSequence::new(sequence))
            .unwrap()
            .unwrap();
        projection.apply(&record).unwrap();
        println!("{}", serde_json::to_string(&record).unwrap());
    }
    assert!(
        journal
            .read(scope(), TaskSequence::new(2))
            .unwrap()
            .is_none()
    );
    let mut bytes = [0; 5];
    let reference = BlobRef {
        digest: Digest::of(b"input"),
        bytes: 5,
        schema: 1,
    };
    assert_eq!(
        journal.read_blob(scope(), reference, &mut bytes).unwrap(),
        5
    );
    assert_eq!(&bytes, b"input");
    println!("{projection:?}");
    drop(journal);
    let store = store(path, clock());
    let request =
        OutcomeReplayRequest::new(process_scope(), 0, NonZeroU16::new(10).unwrap()).unwrap();
    let page = block_on(store.read_outcomes(&request)).unwrap();
    assert_eq!(page.outcomes, commit(false).outcomes);
    println!("{}", serde_json::to_string(&page).unwrap());
    let digest = ContentDigest::sha256(b"retained payload");
    let payload = block_on(store.read_payload(&process_scope(), digest)).unwrap();
    assert_eq!(payload, b"retained payload");
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3);
    let path = Path::new(&args[2]);
    match args[1].as_str() {
        "write" => {
            assert!(!path.exists());
            let mut journal =
                SqliteTaskJournal::open(path, definition(), SqliteLimits::default()).unwrap();
            let record = intent();
            journal.commit(&record, Some(b"input")).unwrap();
            let mut projection = TaskProjection::new(scope(), definition()).unwrap();
            projection.apply(&record).unwrap();
            let started = projection.next(TaskTick::new(1)).unwrap().unwrap();
            journal.commit(&started, None).unwrap();
            drop(journal);
            let store = store(path, clock());
            let commit = commit(false);
            let receipt = block_on(ProcessStore::commit(&store, &commit)).unwrap();
            receipt.validate_for(&commit).unwrap();
            block_on(store.put_payload(
                &process_scope(),
                ContentDigest::sha256(b"retained payload"),
                b"retained payload",
            ))
            .unwrap();
            let claim = OutboxClaimRequest::new(
                process_scope(),
                id("pri_worker"),
                NonZeroU16::new(1).unwrap(),
            )
            .unwrap();
            let leases = block_on(store.claim(&claim)).unwrap();
            assert_eq!(leases.len(), 1);
            assert_eq!(leases[0].record.action, commit.actions[0]);
            // Retain unacknowledged lease authority and delivery uncertainty.
            drop(store);
            inspect(path);
        }
        "read" => inspect(path),
        "refuse" => {
            assert!(matches!(
                SqliteTaskJournal::open(path, definition(), SqliteLimits::default()),
                Err(TaskPortError::Corrupt)
            ));
            assert!(matches!(
                SqliteProcessStore::open(path, SqliteLimits::default(), clock(), 10, 16),
                Err(PortError::Invariant)
            ));
        }
        _ => panic!("unknown mode"),
    }
}
