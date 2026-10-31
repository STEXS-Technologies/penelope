# Cached task record SELECT — 2026-10-31

`SqliteTaskJournal::read` now uses `prepare_cached` for its existing scoped record SELECT. It rebinds the requested scope and sequence on every call, executes inside the same read transaction as the frontier check, and keeps all column bounds, checksum, scope, sequence, version and retained-frontier validation. Only prepared SQL is retained; decoded records and database results are not cached.

The comparison shows repeatable lower latency for all three measured warmed task-read profiles, at the cost of one additional Rust allocation requesting 88 bytes per call. This is a measured tradeoff, not an allocation reduction or a new overall Penelope slowdown percentage.

## Method

Baseline is `bf951100e2c3084f5a2a904035c9b3933085dcc1`. Only `src/tasks.rs` differed between baseline and candidate builds; benchmark, fixture, tests and other runtime source stayed identical. Both versions passed the same 42-test task integration suite. The original production source was restored after building.

Used unchanged `task_reads` and `task_read_allocations` release benchmarks. A fresh journal commits one typed intent record and its input through the public API. Profiles read that record in canonical form, the same valid JSON padded to 4,096 bytes, and an end-of-log cursor. The fixture changes stored bytes and their checksum using another SQLite connection; frontier position stays fixed. Each profile warms 100 reads. Allocation counters run separately in four fresh processes per revision with sixteen samples per profile. Ordinary timing has no counting allocator, performs 1,000 reads per profile, consumes results with `black_box`, and checks expected results before and after the measured loop. Every measured read unwraps its typed result.

Two timing campaigns each use twelve balanced four-process blocks, alternating ABBA and BAAB. Total: 96 ordinary release processes, 288 profile rows and 288,000 timed reads. CPU 0 affinity applies to the synchronous caller on AMD Ryzen 9 7950X, Linux, Rust 1.99.0; databases reside under project qualification-temp on btrfs. No local builds/tests/analysis ran during measurement. Background load and CPU frequency were not controlled. This measures warmed reads on a one-record history, not first-call cost, arbitrary histories, full application throughput or durability latency.

Geometric ratios are computed within each balanced block, with deterministic 20,000-resample block-bootstrap 95% intervals. These are per-profile intervals without a multiple-comparison adjustment. Negative changes mean lower measured latency.

## Latency

| Campaign | Profile | Baseline ns/read | Candidate ns/read | Change | 95% interval |
| --- | --- | ---: | ---: | ---: | --- |
| 0 | record | 8658.7 | 7188.6 | -16.9788% | [-17.4197%, -16.5129%] |
| 0 | padded | 11194.7 | 9691.3 | -13.4301% | [-13.8553%, -12.9835%] |
| 0 | end | 5774.7 | 4373.0 | -24.2732% | [-24.7387%, -23.8179%] |
| 1 | record | 8684.0 | 7191.5 | -17.1864% | [-17.5988%, -16.7608%] |
| 1 | padded | 11280.9 | 9793.2 | -13.1876% | [-13.7865%, -12.6234%] |
| 1 | end | 5796.3 | 4376.4 | -24.4965% | [-25.0436%, -24.0154%] |

## Allocation tradeoff

All 384 samples have the same respective tuples: baseline `(1 allocation, 0 reallocations, 192 requested bytes)` and candidate `(2, 0, 280)` for every profile. The extra cache-key traffic accompanies reuse of the prepared SQL. Counters cover Rust `System` allocation requests, exclude SQLite C allocations, and do not measure retained/peak memory or the prepared statement's C allocation footprint.

## Correctness and qualification

Existing `task_record_columns_preserve_validation_and_owned_results` warms the read before external mutations and covers 25 representations/faults, including six accepted equivalent JSON forms, record bounds, malformed records, scope/sequence/version substitutions and checksum/type/length faults. It checks exact unchanged table snapshots on reads, repaired rows, held owned results and reopening. Existing task frontier tests cover scope/cursor rebinding and another writer's new tail. These run unchanged for both source versions; no validation is bypassed to obtain the timing result.

Full stable workspace all-target/all-feature qualification (excluding fuzz), SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting, layer checks and a local graph update qualify the final source. Raw logs are retained with the evidence. Debug benchmark timings in qualification logs are not latency evidence.

[Evidence and reproduction scripts](verification-data/2026-10-31-task-read-cache/) retain all samples, exact source snapshots/hashes, benchmark and binary hashes, build/test logs and deterministic analysis. Run `python3 docs/verification-data/2026-10-31-task-read-cache/verify.py` to check the samples and recompute all six latency summaries. The build script temporarily replaces source and restores it in `finally`; use an idle checkout.
