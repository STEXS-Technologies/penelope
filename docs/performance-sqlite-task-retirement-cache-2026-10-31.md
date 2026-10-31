# Cached task retirement query — 2026-10-31

The task journal now prepares its retirement-tombstone SELECT with `prepare_cached`. This shared guard is called by record reads, blob reads, commits, head rebuilds and retirement. Scope is rebound and the query executes on every call, in the same connection/transaction as before. No retirement answer is cached. All existing checks and typed errors remain.

The comparison establishes lower warmed task-record read latency, at a cost of one additional Rust allocation requesting 80 bytes per measured read. Performance of the other guarded operations was not measured here. These percentages compare only with `95ff198`; they are not additive with earlier optimizations or an overall Penelope overhead percentage.

## Reliability

Expanded the existing retirement regression test to open an observer journal before another connection retires a task. Its record/blob reads first warm negative tombstone results. The writer completes and fences the task, then publicly retires it. In three rounds, the warmed observer alternates an unknown scope (`Ok(None)`) with five operations on the retired scope, all requiring `Conflict`: record read, commit, head rebuild, blob read and repeated retirement. Blob caller buffers stay unchanged. Exact snapshots of all five relevant tables preserve retirement identity, runtime fences and the removed history. Reopened-journal checks from the original test remain.

Twenty isolated executions passed, ten stable and ten Rust 1.85.0: 300 observer rejection checks and 60 unknown-scope reads, in addition to warmups and existing reopened/fence assertions. This verifies refresh after a completed external retirement and scope rebinding, not an overlapping retirement/read race.

A deliberate control forced the tombstone query to return false while keeping its parameter binding. The strengthened test failed on the warmed observer's record read: it received an empty history instead of `Conflict`. The control was removed before successful qualification. This establishes detection of a false-negative retirement check, not every possible caching defect.

## Measurement

Baseline is `95ff198bf1238c33c2e556557c43a56643995941`. Only `src/tasks.rs` differs between builds; both use the same expanded 42-test task integration suite, unchanged benchmark/fixture and other runtime source. Both suites passed before measurement. Candidate source was restored after building.

Used existing ordinary release `task_reads` and separate `task_read_allocations` benchmarks. A fresh journal publicly commits a typed intent and input. Profiles read a canonical record, the same valid JSON padded to 4,096 bytes with an externally updated checksum, and an end-of-log cursor. The fixture's frontier position stays fixed. Each profile warms 100 reads. Timing performs 1,000 reads, consumes results with `black_box` and checks expected values before/after the timed loop; every read unwraps its typed result. Allocation probes use four fresh processes per revision and sixteen samples per profile.

Two campaigns each have twelve balanced four-process ABBA/BAAB blocks: 96 ordinary release processes, 288 profile rows and 288,000 timed reads. CPU 0 affinity, AMD Ryzen 9 7950X, Linux, Rust 1.99.0; btrfs project qualification-temp databases. No local builds/tests/analysis ran during timing. Background load and frequency were not controlled. Results cover warmed reads in a one-record history, not initial preparation, arbitrary histories, durable write latency or application throughput.

Geometric ratios within balanced blocks use deterministic 20,000-resample block-bootstrap 95% intervals. Negative means lower latency. These are per-profile intervals without multiplicity adjustment.

| Campaign | Profile | Baseline ns/read | Candidate ns/read | Change | 95% interval |
| --- | --- | ---: | ---: | ---: | --- |
| 0 | record | 7178.9 | 5952.6 | -17.0827% | [-17.5004%, -16.6564%] |
| 0 | padded | 9696.8 | 8496.0 | -12.3841% | [-12.7791%, -11.9823%] |
| 0 | end | 4375.8 | 3213.9 | -26.5531% | [-26.9207%, -26.1867%] |
| 1 | record | 7218.9 | 5902.4 | -18.2369% | [-21.1202%, -16.5101%] |
| 1 | padded | 9841.8 | 8462.3 | -14.0171% | [-16.8376%, -12.3223%] |
| 1 | end | 4389.3 | 3166.3 | -27.8631% | [-30.3298%, -26.2880%] |

All 384 allocation samples agree by revision for all profiles: baseline `(2 allocations, 0 reallocations, 280 requested bytes)`, candidate `(3, 0, 360)`. Counters cover Rust `System` requests, not SQLite C allocations or peak/retained memory. The extra cache-key traffic is retained as an explicit cost of reducing repeated SQL preparation.

## Qualification and reproduction

Stable workspace all-feature unit/integration/doc tests (excluding fuzz) and SQLite all-feature unit/integration tests on Rust 1.85.0 passed. SQLite has 34 unit, 61 process and 42 task tests. Strict workspace Clippy covers all targets/features. Formatting, layer boundaries and the local graph update passed. The two release benchmarks were built for both variants; other benchmark binaries were not rerun as qualification for this change.

[Retained evidence](verification-data/2026-10-31-task-retirement-cache/) includes source snapshots/hashes, the expanded retirement test, all raw samples, binary/benchmark hashes, build/test/control/repeat logs and deterministic analysis. Run `python3 docs/verification-data/2026-10-31-task-retirement-cache/verify.py` to check every sample and recompute six latency summaries. The build script temporarily replaces production source and restores it in `finally`; use an idle checkout.
