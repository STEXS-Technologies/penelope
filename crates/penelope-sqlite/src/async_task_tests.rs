//! Public async task adapter preserves native integrity, bounds and cancellation.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]
use super::*;
use crate::{SqliteLimits, machine_test_support::block_on};
use std::{
    future::Future,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};
#[path = "../tests/support/shared/tasks.rs"]
mod native;
fn fixture(path: &std::path::Path, capacity: u16) -> (SqliteAsyncTaskJournal, TaskRecord) {
    let definition = native::definition(RecoveryMode::RestartFromBeginning, EffectContract::Pure);
    let scope = native::scope(definition, b"input");
    let record = TaskProjection::new(scope, definition)
        .unwrap()
        .propose(
            TaskExecutionEvent::IntentCommitted {
                definition,
                input: BlobRef {
                    digest: Digest::of(b"input"),
                    bytes: 5,
                    schema: 1,
                },
            },
            TaskGeneration::ZERO,
            TaskTick::ZERO,
        )
        .unwrap();
    (
        SqliteTaskJournal::open(path, definition, SqliteLimits::default())
            .unwrap()
            .into_async(capacity)
            .unwrap(),
        record,
    )
}
#[test]
fn native_records_blobs_and_buffer_bounds_are_preserved_through_the_async_view() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.db");
    let (journal, record) = fixture(&path, 2);
    assert_eq!(journal.queue_capacity(), 2);
    assert_eq!(
        block_on(journal.commit(&record, Some(b"input"))).unwrap(),
        record
    );
    assert_eq!(
        block_on(journal.read(record.scope, TaskSequence::ZERO)).unwrap(),
        Some(record)
    );
    let TaskExecutionEvent::IntentCommitted { input, .. } = record.event else {
        panic!("intent")
    };
    let mut small = [0xa5; 4];
    assert_eq!(
        block_on(journal.read_blob(record.scope, input, &mut small)),
        Err(TaskPortError::Exhausted)
    );
    assert_eq!(small, [0xa5; 4]);
    let mut full = [0xa5; 8];
    assert_eq!(
        block_on(journal.read_blob(record.scope, input, &mut full)).unwrap(),
        5
    );
    assert_eq!(&full[..5], b"input");
    assert_eq!(&full[5..], &[0xa5; 3]);
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute("UPDATE task_blobs SET bytes=zeroblob(5)", [])
        .unwrap();
    let mut unchanged = [0xa5; 8];
    assert_eq!(
        block_on(journal.read_blob(record.scope, input, &mut unchanged)),
        Err(TaskPortError::Corrupt)
    );
    assert_eq!(unchanged, [0xa5; 8]);
}
#[test]
fn full_native_worker_queue_refuses_immediately_and_cancelled_commit_still_resolves_by_read() {
    let directory = tempfile::tempdir().unwrap();
    let (journal, record) = fixture(&directory.path().join("tasks.db"), 1);
    let (entered, started) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let blocked = journal
        .worker
        .run(move |_native| {
            entered.send(std::thread::current().id()).unwrap();
            resume.recv().unwrap();
            Ok(())
        })
        .unwrap();
    assert_ne!(
        started.recv_timeout(Duration::from_secs(5)).unwrap(),
        std::thread::current().id()
    );
    let mut canceled = Box::pin(journal.commit(&record, Some(b"input")));
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(
        canceled.as_mut().poll(&mut context),
        Poll::Pending
    ));
    assert_eq!(
        block_on(journal.read(record.scope, TaskSequence::ZERO)),
        Err(TaskPortError::Exhausted)
    );
    drop(canceled);
    release.send(()).unwrap();
    block_on(blocked).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match block_on(journal.read(record.scope, TaskSequence::ZERO)) {
            Ok(Some(found)) => {
                assert_eq!(found, record);
                break;
            }
            Err(TaskPortError::Exhausted) => {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            other => panic!("cancelled accepted commit was not retained: {other:?}"),
        }
    }
}
