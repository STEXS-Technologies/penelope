//! Cross-connection acknowledgement/reclaim must match a complete serial history.
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
    Vec<OutboxLease>,
    write_snapshot::Snapshot,
);

fn history(path: &std::path::Path, race: bool, ack_first: bool) -> History {
    let clock = clock();
    let acknowledger = store(path, clock.clone());
    let reclaimer = store(path, clock.clone());
    block_on(ProcessStore::commit(&acknowledger, &commit(false))).unwrap();
    let first = block_on(acknowledger.claim(&claim())).unwrap().remove(0);
    clock.now.store(111, Ordering::SeqCst);
    // A delayed acknowledgement carries an observation made while the old lease
    // was live. Its exact envelope must still be checked inside the transaction.
    let acknowledge = || {
        block_on(OutboxStore::acknowledge(
            &acknowledger,
            &first,
            LogicalTime(105),
        ))
    };
    let reclaim = || block_on(reclaimer.claim(&claim())).unwrap();
    let (ack, leases) = if race {
        let gate = Barrier::new(2);
        std::thread::scope(|threads| {
            let ack_thread = threads.spawn(|| {
                gate.wait();
                if !ack_first {
                    std::thread::yield_now();
                }
                acknowledge()
            });
            let claim_thread = threads.spawn(|| {
                gate.wait();
                if ack_first {
                    std::thread::yield_now();
                }
                reclaim()
            });
            (ack_thread.join().unwrap(), claim_thread.join().unwrap())
        })
    } else {
        if ack_first {
            let ack = acknowledge();
            (ack, reclaim())
        } else {
            let leases = reclaim();
            (acknowledge(), leases)
        }
    };
    drop(acknowledger);
    drop(reclaimer);
    let connection = Connection::open(path).unwrap();
    let snapshot = write_snapshot::snapshot(&connection, TABLES);
    drop(connection);

    let reopened = store(path, clock);
    if let Some(replacement) = leases.first() {
        assert_eq!(ack, Err(PortError::Conflict));
        assert_ne!(first.token, replacement.token);
        assert_eq!(replacement.record.delivery_attempt, 1);
        assert_eq!(replacement.record.action, first.record.action);
        assert_eq!(
            block_on(OutboxStore::acknowledge(
                &reopened,
                &first,
                LogicalTime(105)
            )),
            Err(PortError::Conflict)
        );
        block_on(OutboxStore::acknowledge(
            &reopened,
            replacement,
            LogicalTime(111),
        ))
        .unwrap();
    } else {
        assert_eq!(ack, Ok(()));
    }
    assert!(block_on(reopened.claim(&claim())).unwrap().is_empty());
    (ack, leases, snapshot)
}

#[test]
fn acknowledgement_racing_reclaim_matches_a_durable_serial_history() {
    let directory = tempfile::tempdir().unwrap();
    let ack_first = history(&directory.path().join("ack-first.db"), false, true);
    let claim_first = history(&directory.path().join("claim-first.db"), false, false);
    assert_eq!(ack_first.0, Ok(()));
    assert!(ack_first.1.is_empty());
    assert_eq!(claim_first.0, Err(PortError::Conflict));
    assert_eq!(claim_first.1.len(), 1);
    assert_ne!(ack_first.2, claim_first.2);
    for index in 0..16 {
        let observed = history(
            &directory.path().join(format!("race-{index}.db")),
            true,
            index % 2 == 0,
        );
        assert!(
            observed == ack_first || observed == claim_first,
            "acknowledgement/reclaim produced a nonserial history: {observed:?}"
        );
    }
}
