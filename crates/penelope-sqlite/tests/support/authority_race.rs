//! Reservation/fence histories must match valid serial orders, including identity.
use super::write_snapshot::{Snapshot, snapshot};
use super::*;
use std::sync::{Arc, Barrier};
const TABLES: &[&str] = &[
    "task_records",
    "task_heads",
    "task_blobs",
    "task_retirements",
    "runtime_generations",
];
#[derive(Clone, Copy, Debug)]
enum Seed {
    Empty,
    Same,
    Different,
}
impl Seed {
    const fn identity(self) -> Option<Digest> {
        match self {
            Self::Empty => None,
            Self::Same | Self::Different => Some(Digest([9; 32])),
        }
    }
    const fn attempted(self) -> Digest {
        if matches!(self, Self::Different) {
            Digest([8; 32])
        } else {
            Digest([9; 32])
        }
    }
}
#[derive(Debug, PartialEq)]
struct History {
    reservation: Result<StartReservation, TaskPortError>,
    fence: Result<Option<Digest>, TaskPortError>,
    rows: Snapshot,
}
fn prepare(path: &std::path::Path, seed: Seed) -> TaskProjection {
    let mut journal = open(path);
    let projection = committed_start(&mut journal);
    if let Some(identity) = seed.identity() {
        assert_eq!(
            journal.reserve_start(request(&projection), identity),
            Ok(StartReservation::Reserved)
        );
    }
    projection
}
fn serial(path: &std::path::Path, seed: Seed, reserve_first: bool) -> History {
    let projection = prepare(path, seed);
    let snapshot_connection = Connection::open(path).unwrap();
    let original_history = snapshot(&snapshot_connection, &TABLES[..4]);
    let mut journal = open(path);
    let (reservation, fence) = if reserve_first {
        let reservation = journal.reserve_start(request(&projection), seed.attempted());
        (
            reservation,
            journal.begin_fence(scope(), projection.generation()),
        )
    } else {
        let fence = journal.begin_fence(scope(), projection.generation());
        (
            journal.reserve_start(request(&projection), seed.attempted()),
            fence,
        )
    };
    let expected_reservation = match (seed, reserve_first) {
        (Seed::Empty, true) => Ok(StartReservation::Reserved),
        (Seed::Same, true) => Ok(StartReservation::AlreadyReserved),
        _ => Err(TaskPortError::Conflict),
    };
    let expected_identity = if matches!(seed, Seed::Empty) && !reserve_first {
        None
    } else {
        Some(Digest([9; 32]))
    };
    assert_eq!(reservation, expected_reservation);
    assert_eq!(fence, Ok(expected_identity));
    assert_eq!(
        snapshot(&snapshot_connection, &TABLES[..4]),
        original_history
    );
    History {
        reservation,
        fence,
        rows: snapshot(&snapshot_connection, TABLES),
    }
}
#[test]
fn contending_reservations_and_fences_preserve_serial_histories_and_stop_identity() {
    let directory = tempfile::tempdir().unwrap();
    for (case, seed) in [Seed::Empty, Seed::Same, Seed::Different]
        .into_iter()
        .enumerate()
    {
        let reserve_first = serial(
            &directory.path().join(format!("reserve-first-{case}.db")),
            seed,
            true,
        );
        let fence_first = serial(
            &directory.path().join(format!("fence-first-{case}.db")),
            seed,
            false,
        );
        for round in 0..8 {
            let path = directory.path().join(format!("race-{case}-{round}.db"));
            let projection = prepare(&path, seed);
            let mut reserving = open(&path);
            let mut fencing = open(&path);
            let barrier = Arc::new(Barrier::new(3));
            let reserve_gate = barrier.clone();
            let reserve = std::thread::spawn(move || {
                reserve_gate.wait();
                reserving.reserve_start(request(&projection), seed.attempted())
            });
            let fence_gate = barrier.clone();
            let fence = std::thread::spawn(move || {
                fence_gate.wait();
                fencing.begin_fence(scope(), projection.generation())
            });
            barrier.wait();
            let connection = Connection::open(&path).unwrap();
            let reservation = reserve.join().unwrap();
            let fenced = fence.join().unwrap();
            let observed = History {
                reservation,
                fence: fenced,
                rows: snapshot(&connection, TABLES),
            };
            let model = if observed == reserve_first && observed == fence_first {
                "both"
            } else if observed == reserve_first {
                "reserve-first"
            } else {
                assert_eq!(
                    observed, fence_first,
                    "unserializable authority history: seed={seed:?} round={round}"
                );
                "fence-first"
            };
            let identity = observed.fence.unwrap();
            let (generation, stored_identity, state): (u32, Option<Vec<u8>>, u32) = connection
                .query_row(
                    "SELECT generation,execution,state FROM runtime_generations",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            let count: u32 = connection
                .query_row("SELECT count(*) FROM runtime_generations", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 1);
            assert_eq!(generation, projection.generation().get());
            assert_eq!(stored_identity, identity.map(|value| value.0.to_vec()));
            assert_eq!(state, 1);
            let mut reopened = open(&path);
            assert_eq!(
                reopened.reserve_start(request(&projection), seed.attempted()),
                Err(TaskPortError::Conflict)
            );
            assert_eq!(
                reopened.confirm_stopped(scope(), projection.generation(), Some(Digest([7; 32]))),
                Err(TaskPortError::Conflict)
            );
            assert_eq!(snapshot(&connection, TABLES), observed.rows);
            reopened
                .confirm_stopped(scope(), projection.generation(), identity)
                .unwrap();
            let stopped = snapshot(&connection, TABLES);
            assert_eq!(
                reopened.begin_fence(scope(), projection.generation()),
                Ok(identity)
            );
            reopened
                .confirm_stopped(scope(), projection.generation(), identity)
                .unwrap();
            assert_eq!(
                reopened.reserve_start(request(&projection), seed.attempted()),
                Err(TaskPortError::Conflict)
            );
            assert_eq!(snapshot(&connection, TABLES), stopped);
            let stopped_state: u32 = connection
                .query_row("SELECT state FROM runtime_generations", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(stopped_state, 2);
            assert_eq!(
                reopened.read(scope(), TaskSequence::ZERO),
                Ok(Some(intent()))
            );
            println!(
                "case={case} round={round} model={model} fenced_identity_preserved=true stopped=true delayed_start_rejected=true"
            );
        }
    }
}
