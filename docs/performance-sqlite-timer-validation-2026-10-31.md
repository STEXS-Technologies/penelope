# Borrowed timer scheduling validation — 2026-10-31

Baseline: `a0dbaf25dc0106882dfe886b5ae080867ed2f041`. Candidate: `b15936a1e2655280093cf1db57993909e3656c4a`.

Timer scheduling used to copy the persisted action/checksum and any existing timer/checksum into temporary vectors before decoding. Cancellation also copied its existing timer and checksum. Both paths now borrow bounded SQLite row BLOBs while decoding and cache their SELECT statements. A shared private `stored_timer` helper returns an owned decoded schedule, never a SQLite row or a retained validation result. Every call rereads durable evidence within the existing immediate transaction; duplicate schedule and cancel semantics, checksums, byte bounds, conflict errors, accounting and mutation guards are preserved. No schema or public API change is involved.

## Allocation evidence

The standalone public-port probe is extended with duplicate scheduling and cancellation retries. Baseline and candidate use identical probe/allocator sources, compiler and dependencies. Four release processes per revision, five cases per process, 16 warmup calls and 32 sampled calls per case produce 1,280 recorded samples. Every sample has the same tuple for its case/revision; none are removed.

| Public operation | Allocations before → after | Reallocations before → after | Requested bytes before → after |
|---|---:|---:|---:|
| duplicate timer schedule | 10 → 8 | 4 → 4 | 1,428 → 840 |
| timer cancellation retry | 8 → 7 | 4 → 4 | 1,009 → 704 |
| outbox acknowledgement | 10 → 10 | 14 → 14 | 2,176 → 2,176 |
| timer acknowledgement | 9 → 9 | 10 → 10 | 1,944 → 1,944 |
| outbox renewal | 11 → 11 | 16 → 16 | 3,608 → 3,608 |

The scope is warm duplicate scheduling and repeated cancellation of a single typed timer, alongside the three unchanged lease-completion fixtures. Fresh insertion and first cancellation are not separately sampled. Population, scheduling and claiming use public ports, with ordinary worker dispatch and WAL/FULL transactions. Final durable state/expiry is checked. Rust allocation requests are counted across caller and worker threads, excluding SQLite's own C allocator; requested bytes are allocation traffic, not peak memory. Instrumented probes establish no latency improvement, throughput result or global Penelope slowdown percentage. Temporary databases use the project qualification directory.

## Reliability evidence

The new integration test warms both validation SELECTs with an initial schedule and duplicate schedule, then changes durable rows through a second connection. Twelve fixtures cover action versus timer evidence with damaged checksums, wrong checksum storage class, oversized checksums, wrong payload storage class, oversized payloads and correctly checksummed but substituted payload identity. Schedule retries must reject every fault; cancellation retries must reject faults in the timer row they authorize against. Exact snapshots of process_usage, process_actions, process_timers and lease_counter must remain unchanged. Reopening must preserve each rejection and those snapshots. This qualifies current-row validation after cached statement reuse, not a data or decoded-record cache.

Ten stable and ten Rust 1.85.0 isolated test processes pass: 20 test executions, 240 fault fixtures and 720 rejected schedule/cancel requests across warm and reopened workers. The entire 55-test process suite also passes with the expanded test against both baseline and candidate.

A mutation control bypasses checksum comparison only in the new stored_timer helper by passing a recomputed checksum to decoding. The first damaged-timer-checksum fixture then incorrectly succeeds and the test fails. The control stops at that first failure; it does not execute every faulty combination. Production source is restored in `finally`, and the restored test passes. The baseline itself passes the new test; the change removes temporary work while preserving existing behavior.

Stable workspace all-target/all-feature tests excluding the fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting and layer checks pass, as does graphify update. Both compilers pass 33 SQLite unit, 55 process and 42 task tests. No fresh local fuzz or optional StateChronicle conformance run is claimed. Debug benches in all-target qualification are functional executions, not controlled timing trials.

[Evidence and reproduction](verification-data/2026-10-31-timer-validation/README.md) contains sources, hashes, allocation samples, qualification and intentional-control logs, repeat scripts, summary generation and a standalone verifier. These fixtures do not establish all possible external corruption recovery, all interleavings or power-loss behavior. Release plans from Downloads are not included.
