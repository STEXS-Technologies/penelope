//! Commit an append while replay's head statement holds its older snapshot.
#![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
use super::*;
use penelope_domain::*;
use rusqlite::functions::FunctionFlags;
use std::{
    future::Future,
    num::NonZeroU16,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        assert!(Instant::now() < deadline, "snapshot operation timed out");
        std::thread::park_timeout(Duration::from_millis(5));
    }
}
fn batch(first: u64) -> AtomicProcessCommit {
    let scope = ProcessScope::new(
        "tnt_snapshot".parse().unwrap(),
        "prc_snapshot".parse().unwrap(),
        "def_snapshot".parse().unwrap(),
        "dfv_one".parse().unwrap(),
        ContentDigest([9; 32]),
    );
    let input = ProcessInput::new(
        scope.tenant_id.clone(),
        scope.process_id.clone(),
        format!("inp_{first}").parse().unwrap(),
        ProcessInputKind::CanonicalEvent,
        ContentDigest([1; 32]),
    );
    let outcomes = (first..first + 2)
        .map(|sequence| {
            ProcessOutcome::new(
                scope.clone(),
                sequence,
                ProcessOutcomeFact::new(
                    format!("out_{sequence}").parse().unwrap(),
                    CausationId::Input(input.input_id.clone()),
                    OutcomeActor::System,
                    LogicalTime(sequence),
                    if sequence == 0 {
                        ProcessOutcomeKind::Started
                    } else {
                        ProcessOutcomeKind::ActionPlanned
                    },
                    ContentDigest([2; 32]),
                ),
            )
        })
        .collect();
    AtomicProcessCommit::new(first, Some(input), outcomes, Vec::new()).unwrap()
}
fn verify(page: &OutcomeReplayPage, request: &OutcomeReplayRequest, history: &[ProcessOutcome]) {
    let expected: Vec<_> = history
        .iter()
        .filter(|outcome| outcome.sequence >= request.from_sequence)
        .take(usize::from(request.limit.get()))
        .cloned()
        .collect();
    let next = request.from_sequence + u64::try_from(expected.len()).unwrap();
    assert_eq!(page.scope, request.scope);
    assert_eq!(page.from_sequence, request.from_sequence);
    assert_eq!(page.outcomes, expected);
    assert_eq!(
        page.next_sequence,
        (next < u64::try_from(history.len()).unwrap()).then_some(next)
    );
}

#[test]
fn replay_keeps_one_snapshot_when_an_external_append_commits_during_head_read() {
    for (profile, seeded, from, limit) in [
        ("empty", false, 0, 4),
        ("whole", true, 0, 4),
        ("page", true, 1, 1),
        ("end", true, 2, 4),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("replay.db");
        let limits = SqliteLimits::default();
        let reader = SqliteProcessStore::open(&path, limits, Arc::new(Clock), 10, 16).unwrap();
        let writer = SqliteProcessStore::open(&path, limits, Arc::new(Clock), 10, 16).unwrap();
        let initial = batch(0);
        let appended = batch(if seeded { 2 } else { 0 });
        if seeded {
            wait(ProcessStore::commit(&reader, &initial))
                .unwrap()
                .validate_for(&initial)
                .unwrap();
        }
        let initial_history = if seeded {
            initial.outcomes.clone()
        } else {
            Vec::new()
        };
        let request = OutcomeReplayRequest::new(
            initial.outcomes.first().unwrap().scope(),
            from,
            NonZeroU16::new(limit).unwrap(),
        )
        .unwrap();
        verify(
            &wait(reader.read_outcomes(&request)).unwrap(),
            &request,
            &initial_history,
        );
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let gate_calls = calls.clone();
        let gate_scope = request.scope.canonical_bytes();
        wait(reader.worker.run(move |connection| {
            connection.create_scalar_function("replay_gate", 0, FunctionFlags::SQLITE_UTF8, move |_context| {
                if gate_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    entered.send(()).map_err(|_closed| rusqlite::Error::InvalidQuery)?;
                    resume.recv_timeout(Duration::from_secs(5)).map_err(|_timeout| rusqlite::Error::InvalidQuery)?;
                }
                Ok(if seeded { rusqlite::types::Value::Integer(0) } else { rusqlite::types::Value::Blob(gate_scope.clone()) })
            }).map_err(|error| db_error(&error))?;
            // Reading main.process_heads pins a snapshot before the callback
            // lets the other connection append. TEMP shadowing is reader-only.
            let view = if seeded {
                "CREATE TEMP VIEW process_heads AS SELECT scope,process_key,next_sequence+replay_gate() AS next_sequence,bytes_used FROM main.process_heads"
            } else {
                "CREATE TEMP VIEW process_heads AS SELECT scope,process_key,next_sequence,bytes_used FROM main.process_heads UNION ALL SELECT replay_gate(),NULL,NULL,NULL WHERE NOT EXISTS(SELECT 1 FROM main.process_heads)"
            };
            connection.execute_batch(view)
                .map_err(|error| db_error(&error))?;
            Ok(())
        }).unwrap()).unwrap();
        let append_request = appended.clone();
        let appending = std::thread::spawn(move || {
            started.recv_timeout(Duration::from_secs(5)).unwrap();
            let receipt = wait(ProcessStore::commit(&writer, &append_request)).unwrap();
            receipt.validate_for(&append_request).unwrap();
            assert!(!receipt.duplicate_input);
            release.send(()).unwrap();
        });
        let pinned = wait(reader.read_outcomes(&request));
        appending.join().unwrap();
        let pinned_calls = calls.load(Ordering::SeqCst);
        assert!(pinned_calls >= 1);
        let pinned = pinned.unwrap();
        verify(&pinned, &request, &initial_history);
        let complete: Vec<_> = initial_history
            .iter()
            .chain(&appended.outcomes)
            .cloned()
            .collect();
        verify(
            &wait(reader.read_outcomes(&request)).unwrap(),
            &request,
            &complete,
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            pinned_calls + usize::from(seeded)
        );
        verify(&pinned, &request, &initial_history);
        drop(reader);
        let reopened = SqliteProcessStore::open(&path, limits, Arc::new(Clock), 10, 16).unwrap();
        let all = OutcomeReplayRequest::new(request.scope.clone(), 0, NonZeroU16::new(4).unwrap())
            .unwrap();
        verify(
            &wait(reopened.read_outcomes(&all)).unwrap(),
            &all,
            &complete,
        );
        let duplicate = wait(ProcessStore::commit(&reopened, &appended)).unwrap();
        duplicate.validate_for(&appended).unwrap();
        assert!(duplicate.duplicate_input);
        println!(
            "profile={profile} append_committed_during_read=true pinned_old_page=true later_new_page=true reopened=true"
        );
    }
}
