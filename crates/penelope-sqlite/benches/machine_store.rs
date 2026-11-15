//! Production machine journal, WAL/FULL, independent connections and scoped CAS.
//! Caller/model preparation, correctness checks and client startup are outside API latency.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic_in_result_fn
)]
use penelope_core::machine::*;
use penelope_ports::machine::*;
use penelope_sqlite::{SqliteLimits, SqliteMachineStore};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Barrier},
    time::Instant,
};
#[allow(dead_code)] // Integration fixtures also serve corruption and process-death tests.
#[path = "../tests/support/machines.rs"]
mod support;
use support::*;
// Debug all-target tests exercise every writer count and invariant. Statistical
// measurements use the complete release profile, enforced by the measurement tool.
const SAMPLES: usize = if cfg!(debug_assertions) { 3 } else { 101 };
const WARMUP: usize = if cfg!(debug_assertions) { 1 } else { 8 };

fn distribution(mut samples: Vec<u128>) -> Value {
    assert_eq!(samples.len(), SAMPLES);
    let raw = samples.clone();
    samples.sort_unstable();
    let at = |percent: usize| samples[(samples.len() * percent).div_ceil(100) - 1];
    json!({"samples": samples.len(), "p50_ns": at(50), "p95_ns": at(95),
           "p99_ns": at(99), "raw_ns": raw})
}
fn io_counters() -> Option<BTreeMap<String, u64>> {
    std::fs::read_to_string("/proc/self/io")
        .ok()?
        .lines()
        .map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_owned(), value.trim().parse().ok()?))
        })
        .collect()
}
fn event_id(round: usize, writer: usize) -> MachineEventId {
    let mut bytes = [0; 32];
    bytes[..8].copy_from_slice(&(round as u64).to_le_bytes());
    bytes[8..16].copy_from_slice(&(writer as u64).to_le_bytes());
    MachineEventId::from_bytes(bytes)
}
struct Attempt {
    writer: usize,
    result: Result<MachineCommitReceipt, MachineStoreError>,
    elapsed_ns: u128,
    finished: Instant,
}
fn race(
    stores: &[SqliteMachineStore<Policy, 2>],
    batches: &[Arc<MachineCommitBatch<2>>],
) -> (Vec<Attempt>, u128) {
    let ready = Barrier::new(stores.len() + 1);
    let go = Barrier::new(stores.len() + 1);
    std::thread::scope(|scope| {
        let threads = stores
            .iter()
            .zip(batches)
            .enumerate()
            .map(|(writer, (store, batch))| {
                let ready = &ready;
                let go = &go;
                let batch = Arc::clone(batch);
                scope.spawn(move || {
                    ready.wait();
                    go.wait();
                    let started = Instant::now();
                    let result = block_on(store.commit(&true, batch));
                    let finished = Instant::now();
                    Attempt {
                        writer,
                        result,
                        elapsed_ns: finished.duration_since(started).as_nanos(),
                        finished,
                    }
                })
            })
            .collect::<Vec<_>>();
        ready.wait(); // All clients exist before the measured release.
        let release = Instant::now();
        go.wait();
        let attempts = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        let finish = attempts
            .iter()
            .map(|attempt| attempt.finished)
            .max()
            .unwrap();
        (attempts, finish.duration_since(release).as_nanos())
    })
}
fn profile(parent: &Path, writers: usize) -> Value {
    let directory = tempfile::tempdir_in(parent).unwrap();
    let path = directory.path().join("machine.db");
    let opened = Instant::now();
    let stores = (0..writers).map(|_| open(&path)).collect::<Vec<_>>();
    block_on(stores[0].bootstrap(&true, genesis())).unwrap();
    let open_and_bootstrap_ns = opened.elapsed().as_nanos();
    let sql =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mode: String = sql
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    let io_before = io_counters();
    let mut commits = Vec::with_capacity(SAMPLES);
    let mut losers = Vec::with_capacity(SAMPLES);
    let mut duplicates = Vec::with_capacity(SAMPLES);
    let mut snapshots = Vec::with_capacity(SAMPLES);
    let mut round_times = Vec::with_capacity(SAMPLES);
    for round in 0..WARMUP + SAMPLES {
        let mut owner = restore(&stores[0]);
        assert_eq!(*owner.read().1, 5 + round as u64);
        assert_eq!(owner.read().0.revision, MachineRevision::new(round as u64));
        let batches = (0..writers)
            .map(|writer| batch_id(&mut owner, 1, event_id(round, writer)))
            .collect::<Vec<_>>();
        let (attempts, elapsed_ns) = race(&stores, &batches);
        let mut winner = None;
        let mut stale = Vec::new();
        for attempt in attempts {
            match attempt.result {
                Ok(receipt) => {
                    assert_eq!(receipt.disposition, MachineCommitDisposition::Committed);
                    assert!(
                        winner
                            .replace((attempt.writer, receipt, attempt.elapsed_ns))
                            .is_none()
                    );
                }
                Err(error) => {
                    assert_eq!(error, MachineStoreError::StaleHead);
                    stale.push(attempt.elapsed_ns);
                }
            }
        }
        assert_eq!(stale.len(), writers - 1);
        let (writer, receipt, commit_ns) = winner.unwrap();
        assert_eq!(receipt.event, event_id(round, writer));
        assert_eq!(receipt.head.scope, SCOPE);
        assert_eq!(
            receipt.head.revision,
            MachineRevision::new(round as u64 + 1)
        );
        assert_eq!(receipt.identity, batches[writer].record().identity());
        let retry_started = Instant::now();
        let duplicate = block_on(stores[0].commit(&true, Arc::clone(&batches[writer]))).unwrap();
        let duplicate_ns = retry_started.elapsed().as_nanos();
        assert_eq!(
            duplicate,
            MachineCommitReceipt {
                disposition: MachineCommitDisposition::AlreadyCommitted,
                ..receipt
            }
        );
        let snapshot_started = Instant::now();
        let snapshot = block_on(stores[0].load_head(&true, SCOPE))
            .unwrap()
            .unwrap();
        let snapshot_ns = snapshot_started.elapsed().as_nanos();
        assert_eq!(snapshot.head(), receipt.head);
        assert_eq!(snapshot.state(), (6 + round as u64).to_le_bytes());
        assert_eq!(snapshot.last().unwrap().identity, receipt.identity);
        let stored = block_on(stores[0].lookup(&true, SCOPE, receipt.event))
            .unwrap()
            .unwrap();
        assert_eq!(stored.receipt(), duplicate);
        assert_eq!(stored.batch().event(), 1_u64.to_le_bytes());
        assert_eq!(stored.batch().intents().count(), 1);
        assert_eq!(
            stored.batch().intents().next().unwrap().1,
            (6 + round as u64).to_le_bytes()
        );
        if round >= WARMUP {
            commits.push(commit_ns);
            // One percentile per race; not a pooled percentile over all losers.
            if let Some(slowest) = stale.into_iter().max() {
                losers.push(slowest);
            }
            duplicates.push(duplicate_ns);
            snapshots.push(snapshot_ns);
            round_times.push(elapsed_ns);
        }
    }
    let total = WARMUP + SAMPLES;
    let counts: (u32, u32) = sql
        .query_row(
            "SELECT (SELECT count(*) FROM machine_events),(SELECT count(*) FROM machine_intents)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (total as u32, total as u32));
    let integrity: String = sql
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let wal_bytes = std::fs::metadata(wal).unwrap().len();
    let release_to_last_finish_ns: u128 = round_times.iter().sum();
    let slowest_stale = if writers == 1 {
        Value::Null
    } else {
        distribution(losers)
    };
    json!({
        "writers": writers, "warmup_rounds": WARMUP, "measured_rounds": SAMPLES,
        "one_committed_per_round": true, "one_intent_per_committed_event": true,
        "open_and_bootstrap_ns": open_and_bootstrap_ns,
        "commit": distribution(commits), "slowest_stale_per_round": slowest_stale,
        "exact_retry": distribution(duplicates), "snapshot_read": distribution(snapshots),
        "release_to_last_finish": distribution(round_times),
        "commits_per_measured_race_second": SAMPLES as u128 * 1_000_000_000 / release_to_last_finish_ns,
        "attempts_per_measured_race_second": (SAMPLES * writers) as u128 * 1_000_000_000 / release_to_last_finish_ns,
        "wal_bytes_at_end": wal_bytes, "database_bytes_at_end": std::fs::metadata(&path).unwrap().len(),
        "process_io_before": io_before, "process_io_after": io_counters(),
        "committed_rows_including_warmup": counts.0, "intent_rows_including_warmup": counts.1,
        "integrity_check": integrity
    })
}
fn main() {
    let parent = std::env::var_os("PENELOPE_BENCH_DIR")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let profiles = [1, 2, 8, 32]
        .into_iter()
        .map(|writers| profile(&parent, writers))
        .collect::<Vec<_>>();
    println!(
        "{}",
        json!({
            "schema": 1, "benchmark": "machine_store", "sqlite_version": rusqlite::version(),
        "measurement_mode": if cfg!(debug_assertions) { "debug-smoke" } else { "release-101" },
            "journal_mode": "WAL", "synchronous": "FULL", "data_parent": parent,
            "state_bytes": 8, "event_bytes": 8, "intent_bytes": 8, "profiles": profiles,
            "latency_scope": "Individual authorized API calls including async enqueue/wake; prepared model batches and correctness checks excluded",
            "race_throughput_scope": "Barrier release to last completed CAS call, excluding client startup/join and model preparation; one commit and writers-1 stale refusals per race",
            "snapshot_policy": "Every atomic commit retains required next-state bytes. Head-snapshot reads are measured separately; no unsafe state-free commit mode",
            "io_scope": "Raw Linux process counters around each whole profile including warmup and verification; unavailable on other hosts. Not per-operation disk traffic",
            "allocation_scope": "Not instrumented; external whole-process peak RSS only",
            "cache_policy": "OS/page caches uncontrolled; no cold-cache claim",
            "effects": "Committed intents retained; no admission, dispatch or external execution"
        })
    );
}
