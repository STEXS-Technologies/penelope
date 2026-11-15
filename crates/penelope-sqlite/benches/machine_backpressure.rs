//! Saturate the public bounded journal queue behind a real SQLite writer lock.
//! First-poll refusal latency is distinct from deliberately gated completion.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic_in_result_fn
)]
use penelope_core::{machine::*, task::Digest};
use penelope_ports::machine::*;
use penelope_sqlite::{SqliteLimits, SqliteMachineStore};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Instant,
};
#[allow(dead_code)] // Same model/codec/driver used by journal integration tests.
#[path = "../tests/support/machines.rs"]
mod support;
use support::*;
const SAMPLES: usize = if cfg!(debug_assertions) { 3 } else { 101 };
const WARMUP: usize = if cfg!(debug_assertions) { 1 } else { 8 };
fn distribution(mut values: Vec<u128>) -> Value {
    assert!(!values.is_empty());
    let raw = values.clone();
    values.sort_unstable();
    let at = |percent: usize| values[(values.len() * percent).div_ceil(100) - 1];
    json!({"samples": values.len(), "p50_ns": at(50), "p95_ns": at(95), "p99_ns": at(99), "raw_ns": raw})
}
fn profile(parent: &Path, capacity: u16, requests: usize) -> Value {
    let directory = tempfile::tempdir_in(parent).unwrap();
    let path = directory.path().join("queue.db");
    let store = SqliteMachineStore::<_, 2>::open(
        &path,
        SqliteLimits::default(),
        PAYLOADS,
        Policy,
        capacity,
    )
    .unwrap();
    assert_eq!(store.queue_capacity(), capacity);
    block_on(store.bootstrap(&true, genesis())).unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    let mode: String = sql
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    let mut context = Context::from_waker(Waker::noop());
    let mut refused_times = Vec::new();
    let mut admission_times = Vec::new();
    let mut completion_times = Vec::new();
    let mut rounds = Vec::with_capacity(SAMPLES);
    for round in 0..WARMUP + SAMPLES {
        let mut owner = restore(&store);
        let batches: Vec<_> = (0..requests)
            .map(|request| {
                let mut identity = [0; 16];
                identity[..8].copy_from_slice(&(round as u64).to_le_bytes());
                identity[8..].copy_from_slice(&(request as u64).to_le_bytes());
                batch_id(
                    &mut owner,
                    1,
                    MachineEventId::from_bytes(Digest::of(&identity).0),
                )
            })
            .collect();
        // Every commit must acquire the writer lock before head/CAS checks.
        // This pins the active worker while we fill its bounded queue, without
        // sleeping, accessing private queue state or changing busy timeouts.
        sql.execute_batch("BEGIN IMMEDIATE").unwrap();
        let mut admitted = Vec::new();
        let mut refused = 0;
        for (request, batch) in batches.iter().enumerate() {
            let started = Instant::now();
            let mut future = store.commit(&true, Arc::clone(batch));
            let result = future.as_mut().poll(&mut context);
            let elapsed = started.elapsed().as_nanos();
            match result {
                Poll::Pending => {
                    if round >= WARMUP {
                        admission_times.push(elapsed);
                    }
                    admitted.push((request, started, future));
                }
                Poll::Ready(Err(MachineStoreError::Capacity)) => {
                    refused += 1;
                    if round >= WARMUP {
                        refused_times.push(elapsed);
                    }
                }
                unexpected => {
                    panic!("writer lock must hold accepted calls pending: {unexpected:?}")
                }
            }
        }
        assert!((1..=usize::from(capacity) + 1).contains(&admitted.len()));
        assert_eq!(admitted.len() + refused, requests);
        assert!(refused >= requests - usize::from(capacity) - 1);
        let admitted_count = admitted.len();
        sql.execute_batch("ROLLBACK").unwrap();
        let mut winner = None;
        let mut stale = 0;
        for (request, started, future) in admitted {
            let result = block_on(future);
            let elapsed = started.elapsed().as_nanos();
            match result {
                Ok(receipt) => {
                    assert_eq!(receipt.disposition, MachineCommitDisposition::Committed);
                    assert_eq!(receipt.identity, batches[request].record().identity());
                    assert!(winner.replace((request, receipt)).is_none());
                }
                Err(MachineStoreError::StaleHead) => stale += 1,
                unexpected => panic!("accepted queue call failed: {unexpected:?}"),
            }
            if round >= WARMUP {
                completion_times.push(elapsed);
            }
        }
        assert_eq!(stale + 1, admitted_count);
        let (request, receipt) = winner.unwrap();
        let retry = block_on(store.commit(&true, Arc::clone(&batches[request]))).unwrap();
        assert_eq!(
            retry,
            MachineCommitReceipt {
                disposition: MachineCommitDisposition::AlreadyCommitted,
                ..receipt
            }
        );
        let snapshot = block_on(store.load_head(&true, SCOPE)).unwrap().unwrap();
        assert_eq!(snapshot.head(), receipt.head);
        assert_eq!(snapshot.head().revision.get(), round as u64 + 1);
        assert_eq!(snapshot.state(), (6 + round as u64).to_le_bytes());
        let retained = block_on(store.lookup(&true, SCOPE, receipt.event))
            .unwrap()
            .unwrap();
        assert_eq!(
            retained.batch().record().identity(),
            batches[request].record().identity()
        );
        assert_eq!(retained.batch().event(), batches[request].event());
        assert_eq!(retained.batch().state(), batches[request].state());
        assert!(retained.batch().intents().eq(batches[request].intents()));
        assert_eq!(retained.batch().intents().count(), 1);
        if round >= WARMUP {
            rounds.push(json!({"admitted": admitted_count, "refused": refused, "stale": stale, "committed": 1}));
        }
    }
    let counts: (u32, u32) = sql
        .query_row(
            "SELECT (SELECT count(*) FROM machine_events),(SELECT count(*) FROM machine_intents)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        counts,
        ((SAMPLES + WARMUP) as u32, (SAMPLES + WARMUP) as u32)
    );
    let integrity: String = sql
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    json!({"queue_capacity": capacity, "requests_per_round": requests, "warmup_rounds": WARMUP, "measured_rounds": SAMPLES,
        "refusal_first_poll": distribution(refused_times), "accepted_first_poll": distribution(admission_times),
        "accepted_gated_completion": distribution(completion_times), "rounds": rounds,
        "event_rows_including_warmup": counts.0, "intent_rows_including_warmup": counts.1, "integrity_check": integrity})
}
fn main() {
    let parent = std::env::var_os("PENELOPE_BENCH_DIR")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let profiles: Vec<_> = [1, 8]
        .into_iter()
        .flat_map(|capacity| {
            [32, 128]
                .into_iter()
                .map(move |requests| (capacity, requests))
        })
        .map(|(capacity, requests)| profile(&parent, capacity, requests))
        .collect();
    println!(
        "{}",
        json!({"schema": 1, "benchmark": "machine_backpressure", "sqlite_version": rusqlite::version(),
        "measurement_mode": if cfg!(debug_assertions) { "debug-smoke" } else { "release-101" },
        "journal_mode": "WAL", "synchronous": "FULL", "profiles": profiles,
        "refusal_scope": "Authorized public commit construction and first poll, including enqueue attempt; known full-queue Capacity refusal while external writer lock remains held",
        "completion_scope": "Accepted call to observed completion, including intentional writer-lock gating, burst submission and sequential caller draining; not normal store latency",
        "sampling": "Pooled chronological per-call samples; separate population sizes for accepted/refused calls; model preparation and correctness checks excluded",
        "limits": "Existing store limits/timeouts unchanged; up to capacity queued plus one active worker; independent worker scheduling controls exact occupancy",
        "effects": "One durable transition/intent per burst; no native task execution or external effect; allocations and stack not instrumented"})
    );
}
