# Borrowed exact process commit verification — 2026-10-31

Baseline: `1ed0b333b7248022f65a13f5c00826a75df17d14`. Candidate: `68c9eac2bd3c8dc5b164a1d8101c072e717e10b9`.

Exact process commit retries reconstruct a receipt only after verifying its retained outcomes, actions, consumed input and source binding. The verification path used to copy each retained record and checksum into temporary vectors before decoding. It now borrows bounded BLOBs from the current SQLite row and uses cached validation SELECTs. Outcome/action statements stay prepared across their respective loops; each iteration executes the query again for that constituent. Input evidence has its own scoped statement. Rows and statements finish inside the verification function before the retry returns.

This changes temporary work in exact retries, without weakening receipt verification or changing normal first-commit writes. All existing lookup predicates, checksums, per-record bounds, decoded equality, consumed-input digest/frontier binding and source-event binding checks remain. Every retry rereads durable rows inside its existing immediate transaction; no decoded record or successful verification is cached across calls. No schema or public API changes are made.

## Allocation evidence

A release probe populates a database using a typed public atomic commit, then retries that exact commit through the ordinary database worker. Fixtures contain one outcome and one consumed input, with either one or 16 actions. Each process warms 16 retries and samples 32. Four separate processes per revision produce 512 allocation samples; every sample for a revision/case has the same tuple and none are discarded. Baseline and candidate use identical probe and allocator sources and the same toolchain/dependencies.

| Actions per committed batch | Allocations before → after | Reallocations before → after | Requested bytes before → after |
|---|---:|---:|---:|
| 1 | 21 → 18 | 16 → 16 | 7,075 → 6,392 |
| 16 | 66 → 33 | 78 → 78 | 32,143 → 25,640 |

Every returned receipt must validate against its commit and be marked duplicate. Final durable counts must remain one outcome, the original action count, one commit receipt row and one consumed inbox input. The fixture uses normal worker dispatch and WAL/FULL transactions in project qualification temporary storage. Rust System allocation/reallocation requests across caller and worker threads are counted; SQLite's C allocator and peak/live memory are outside the counter. Requested bytes are cumulative allocation traffic, not retained memory. These warm exact-retry fixtures do not measure first-commit latency, multi-outcome scaling or a universal overhead percentage. No wall-clock improvement follows from instrumented allocation runs alone.

## Reliability qualification

The constituent-integrity regression now warms an exact retry before changing durable evidence via a second connection. Its fixture has two outcomes and two actions; damage targets the later outcome/action so verification must get beyond a valid first constituent. Three record types (outcome, action, input) each cover deletion, correctly checksummed substitution, damaged checksum, wrong payload storage class, oversized payload, wrong checksum storage class and oversized checksum. Head deletion and frontier substitution add two cases: 23 fault fixtures altogether.

Every retry must reject with Invariant, and exact snapshots of all ten process/lease-counter tables must remain unchanged. A reopened worker must reject again without row changes. Twenty isolated regression processes, ten stable and ten Rust 1.85.0, pass 460 fault fixtures and 920 rejected retries. The complete 55-test process suite also passes against both baseline and candidate with this expanded test.

A mutation control bypasses outcome checksum comparison by feeding decoding a recomputed checksum. The outcome/checksum fixture then incorrectly returns a receipt and fails the test. The control stops at that first intentional failure; it does not execute every faulty case. Production source restoration is guaranteed by `finally`, and the restored integrity test passes. The baseline passes the same regression: this optimization preserves the existing protection.

Stable workspace all-target/all-feature tests excluding the fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting and layer checks pass. Both compilers pass 33 SQLite unit, 55 process and 42 task tests. Graphify update also passes. No fresh local fuzz or optional StateChronicle conformance run is claimed; debug benches included in all-target qualification are functional executions, not controlled timing measurements.

[Evidence and reproduction](verification-data/2026-10-31-commit-validation/README.md) includes raw allocation samples, sources and hashes, the intentional failed control, repeated regressions, qualification logs, summary generation and a standalone verifier. This does not prove all corruption recovery, all interleavings or power-loss behavior. Release plans from Downloads are not included.
