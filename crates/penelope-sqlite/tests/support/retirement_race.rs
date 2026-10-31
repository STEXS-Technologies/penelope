//! Retirement may only cross the exact durable stop-confirmation boundary.
use super::write_snapshot::snapshot;
use super::*;
use std::sync::{Arc, Barrier};

const TABLES: &[&str] = &[
    "task_records",
    "task_heads",
    "task_blobs",
    "task_retirements",
    "runtime_generations",
];

fn prepare(path: &std::path::Path, identity: Option<Digest>) -> TaskProjection {
    let mut journal = open(path);
    let mut projection = committed_start(&mut journal);
    if let Some(execution) = identity {
        assert_eq!(
            journal.reserve_start(request(&projection), execution),
            Ok(StartReservation::Reserved)
        );
    }
    let complete = projection
        .propose(
            TaskExecutionEvent::Completed {
                output: BlobRef {
                    digest: Digest::of(b"done"),
                    bytes: 4,
                    schema: 1,
                },
            },
            projection.generation(),
            TaskTick::new(2),
        )
        .unwrap();
    journal.commit(&complete, Some(b"done")).unwrap();
    projection.apply(&complete).unwrap();
    assert_eq!(
        journal.begin_fence(scope(), projection.generation()),
        Ok(identity)
    );
    let connection = Connection::open(path).unwrap();
    let before = snapshot(&connection, TABLES);
    assert_eq!(
        journal.retire_task(scope(), projection.next_sequence()),
        Err(TaskPortError::Conflict)
    );
    assert_eq!(snapshot(&connection, TABLES), before);
    projection
}

#[test]
fn retirement_racing_stop_confirmation_preserves_identity_and_tombstone() {
    let directory = tempfile::tempdir().unwrap();
    for (case, identity) in [None, Some(Digest([9; 32]))].into_iter().enumerate() {
        let serial_path = directory.path().join(format!("serial-{case}.db"));
        let serial_projection = prepare(&serial_path, identity);
        let mut serial = open(&serial_path);
        serial
            .confirm_stopped(scope(), serial_projection.generation(), identity)
            .unwrap();
        let serial_receipt = serial
            .retire_task(scope(), serial_projection.next_sequence())
            .unwrap();
        assert_eq!(
            serial_receipt.next_sequence,
            serial_projection.next_sequence()
        );
        for round in 0..8 {
            let path = directory.path().join(format!("retire-{case}-{round}.db"));
            let projection = prepare(&path, identity);
            let connection = Connection::open(&path).unwrap();
            let retained = snapshot(&connection, &TABLES[..4]);
            let last_digest: Vec<u8> = connection
                .query_row("SELECT last_digest FROM task_heads", [], |row| row.get(0))
                .unwrap();
            let mut retiring = open(&path);
            let mut stopping = open(&path);
            let gate = Arc::new(Barrier::new(3));
            let retire_gate = gate.clone();
            let retire = std::thread::spawn(move || {
                retire_gate.wait();
                retiring.retire_task(scope(), projection.next_sequence())
            });
            let stop_gate = gate.clone();
            let stop = std::thread::spawn(move || {
                stop_gate.wait();
                stopping.confirm_stopped(scope(), projection.generation(), identity)
            });
            gate.wait();
            let retired = retire.join().unwrap();
            assert_eq!(stop.join().unwrap(), Ok(()));
            let mut reopened = open(&path);
            let model = if retired == Err(TaskPortError::Conflict) {
                assert_eq!(snapshot(&connection, &TABLES[..4]), retained);
                "retire-first"
            } else {
                assert!(retired.is_ok(), "unexpected retirement result: {retired:?}");
                "stop-first"
            };
            let receipt = match retired {
                Ok(receipt) => receipt,
                Err(TaskPortError::Conflict) => reopened
                    .retire_task(scope(), projection.next_sequence())
                    .unwrap(),
                Err(error) => panic!("unexpected retirement failure: {error:?}"),
            };
            assert_eq!(receipt.scope, scope());
            assert_eq!(receipt.next_sequence, projection.next_sequence());
            assert_eq!(receipt.last_record_digest.0.as_slice(), last_digest);
            let counts: (u32, u32, u32, u32) = connection
                .query_row("SELECT (SELECT count(*) FROM task_records),(SELECT count(*) FROM task_heads),(SELECT count(*) FROM task_blobs),(SELECT count(*) FROM task_retirements)", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)))
                .unwrap();
            assert_eq!(counts, (0, 0, 0, 1));
            let authority: (Option<Vec<u8>>, u32) = connection
                .query_row(
                    "SELECT execution,state FROM runtime_generations",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(authority, (identity.map(|id| id.0.to_vec()), 2));
            let authority_count: u32 = connection
                .query_row("SELECT count(*) FROM runtime_generations", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(authority_count, 1);
            let tombstone: (u32, Vec<u8>) = connection
                .query_row(
                    "SELECT next_sequence,last_digest FROM task_retirements",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(tombstone, (projection.next_sequence().get(), last_digest));
            let final_rows = snapshot(&connection, TABLES);
            assert_eq!(
                reopened.confirm_stopped(scope(), projection.generation(), Some(Digest([7; 32]))),
                Err(TaskPortError::Conflict)
            );
            assert_eq!(snapshot(&connection, TABLES), final_rows);
            assert_eq!(
                reopened.confirm_stopped(scope(), projection.generation(), identity),
                Ok(())
            );
            assert_eq!(
                reopened.commit(&intent(), Some(b"input")),
                Err(TaskPortError::Conflict)
            );
            assert_eq!(
                reopened.read(scope(), TaskSequence::ZERO),
                Err(TaskPortError::Conflict)
            );
            assert_eq!(
                reopened.retire_task(scope(), projection.next_sequence()),
                Err(TaskPortError::Conflict)
            );
            assert!(
                reopened
                    .reserve_start(request(&projection), Digest([9; 32]))
                    .is_err()
            );
            assert_eq!(snapshot(&connection, TABLES), final_rows);
            drop(reopened);
            let mut final_reopen = open(&path);
            assert_eq!(
                final_reopen.read(scope(), TaskSequence::ZERO),
                Err(TaskPortError::Conflict)
            );
            assert_eq!(snapshot(&connection, TABLES), final_rows);
            println!(
                "case={case} round={round} model={model} stopped_identity_preserved=true retirement_tombstone_durable=true"
            );
        }
    }
}
