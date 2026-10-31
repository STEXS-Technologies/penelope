//! Acknowledgement cannot overwrite a concurrently committed cancellation.
use super::*;
use std::sync::Barrier;

const TABLES: &[&str] = &[
    "process_usage",
    "process_heads",
    "process_outcomes",
    "process_inbox",
    "process_sources",
    "process_commits",
    "process_actions",
    "process_timers",
    "lease_counter",
    "process_payloads",
];
type History = (
    Result<(), PortError>,
    Result<(), PortError>,
    write_snapshot::Snapshot,
);

fn history(path: &std::path::Path, race: bool, cancel_first: bool) -> History {
    let clock = clock();
    let canceller = store(path, clock.clone());
    let acknowledger = store(path, clock.clone());
    let commit = commit(true);
    block_on(ProcessStore::commit(&canceller, &commit)).unwrap();
    let timer = TimerSchedule {
        action: commit.actions[0].clone(),
        due_at: LogicalTime(100),
    };
    block_on(canceller.schedule(&timer)).unwrap();
    let lease = block_on(acknowledger.claim_due(&timer_claim(100)))
        .unwrap()
        .remove(0);
    let cancel = || block_on(canceller.cancel(&timer));
    let acknowledge = || {
        block_on(TimerClaimStore::acknowledge(
            &acknowledger,
            &lease,
            LogicalTime(100),
        ))
    };
    let (cancelled, acknowledged) = if race {
        let barrier = Barrier::new(2);
        std::thread::scope(|threads| {
            let cancellation = threads.spawn(|| {
                barrier.wait();
                if !cancel_first {
                    std::thread::yield_now();
                }
                cancel()
            });
            let acknowledgement = threads.spawn(|| {
                barrier.wait();
                if cancel_first {
                    std::thread::yield_now();
                }
                acknowledge()
            });
            (
                cancellation.join().unwrap(),
                acknowledgement.join().unwrap(),
            )
        })
    } else if cancel_first {
        let cancelled = cancel();
        (cancelled, acknowledge())
    } else {
        let acknowledged = acknowledge();
        (cancel(), acknowledged)
    };
    drop(canceller);
    drop(acknowledger);
    let connection = Connection::open(path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT state FROM process_timers", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let snapshot = write_snapshot::snapshot(&connection, TABLES);
    drop(connection);
    let reopened = store(path, clock);
    assert_eq!(
        block_on(TimerClaimStore::acknowledge(
            &reopened,
            &lease,
            LogicalTime(100)
        )),
        Err(PortError::Conflict)
    );
    block_on(reopened.cancel(&timer)).unwrap();
    block_on(reopened.schedule(&timer)).unwrap();
    assert!(
        block_on(reopened.claim_due(&timer_claim(1000)))
            .unwrap()
            .is_empty()
    );
    let reopened_connection = Connection::open(path).unwrap();
    assert_eq!(
        write_snapshot::snapshot(&reopened_connection, TABLES),
        snapshot
    );
    (cancelled, acknowledged, snapshot)
}

#[test]
fn cancellation_racing_acknowledgement_keeps_the_timer_terminal() {
    let directory = tempfile::tempdir().unwrap();
    let cancel_first = history(&directory.path().join("cancel-first.db"), false, true);
    let ack_first = history(&directory.path().join("ack-first.db"), false, false);
    assert_eq!(cancel_first.0, Ok(()));
    assert_eq!(cancel_first.1, Err(PortError::Conflict));
    assert_eq!(ack_first.0, Ok(()));
    assert_eq!(ack_first.1, Ok(()));
    assert_eq!(cancel_first.2, ack_first.2);
    for index in 0..16 {
        let observed = history(
            &directory.path().join(format!("race-{index}.db")),
            true,
            index % 2 == 0,
        );
        assert!(
            observed == cancel_first || observed == ack_first,
            "nonserial timer history: {observed:?}"
        );
    }
}
