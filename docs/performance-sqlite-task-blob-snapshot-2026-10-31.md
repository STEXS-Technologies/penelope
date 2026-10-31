# Atomic task blob reads across retirement — 2026-10-31

Fixed a real read/retirement race in `SqliteTaskJournal::read_blob`. Previously the retirement check and blob SELECT were independent statements. A task could retire between them: the first query saw no tombstone, retirement deleted its blobs, and the second query returned `Corrupt` for a valid lifecycle transition.

The public blob read now selects tombstone status and scoped blob bytes in one cached SQL statement, giving them one SQLite snapshot. A read pinned before retirement returns its verified blob; a later read returns `Conflict`. It still validates definition and caller-buffer size before SQL, prioritizes retirement before the configured blob bound, validates stored type/size/digest, and copies into the caller buffer only after verification. Reconstruction keeps its existing transaction-scoped blob statement. No blob data or retirement results are cached between operations.

## Deterministic reproduction

The new unit test publicly commits intent, start and completion, then warms the blob reader. A test-only TEMP view shadows the reader's tombstone table. It reads real main-database tombstones, then calls a scalar gate that returns a nonmatching empty scope. The gate signals a second thread and waits for its public `retire_task` commit. Both connections use the real WAL database. No production failpoint or new dependency feature is introduced.

The main-table branch establishes a reader snapshot before the gated retirement. In the old two-statement implementation, that snapshot ends with the tombstone query; the blob query opens a newer snapshot and incorrectly returns `Corrupt`. The retained baseline run reproduces exactly this failure. In the fixed single-statement implementation, blob data stays in the earlier snapshot and returns `Ok(4)`, with correct bytes and unchanged sentinel tail. A subsequent read and a reopened journal return `Conflict` without modifying their buffers. The gate executes exactly once, the writer's retirement receipt matches its scope/frontier, and durable counts show zero records/blobs/heads with one tombstone.

Twenty isolated fixed runs passed, ten stable and ten Rust 1.85.0. These are gated overlapping read/retirement executions, not probability-based stress. This does not claim process-kill, power-loss, or every possible lifecycle interleaving coverage.

## Performance comparison

Baseline runtime is `234c23f0ab98f241219be84b12ef4143581a4285`; the baseline source snapshot also contains the shared test-only module declaration. Only runtime `src/tasks.rs` differs between release builds. Tests, fixture and all other runtime source stay identical. Both versions pass the same existing 43-test task integration suite; the new race unit test intentionally fails on baseline and passes on the fix. Candidate source was restored after building.

Used unchanged ordinary release `task_blobs` and separate `task_blob_allocations` benchmarks. Fresh journals publicly commit typed intents and blobs. Healthy/corrupt profiles have declared sizes 0, 32, 4,096 and 1,048,576 bytes. Corrupt nonempty stored bytes have equal-length wrong contents; the corrupt zero-byte reference has one stored byte. Sixteen warmups precede each profile. Timing checks the expected typed result on every read; caller-buffer and retained stored-byte checks occur outside timing. Corrupt buffers retain their sentinel contents.

Two campaigns each contain twelve balanced four-process ABBA/BAAB blocks: 96 ordinary release processes, 768 profile rows and 600,576 measured reads. Small profiles use 1,000 reads; 1 MiB profiles use 128. CPU 0 affinity, AMD Ryzen 9 7950X, Linux, Rust 1.99.0; btrfs project qualification-temp databases. No local builds/tests/analysis ran during timing. Background load and CPU frequency were not controlled. Results cover warmed fixture reads, not initial preparation or overall application performance.

Per-block geometric ratios use deterministic 20,000-resample block-bootstrap 95% intervals. Negative means lower latency. Intervals are per profile, without multiplicity correction. All measured profiles are retained.

| Campaign | Declared bytes | Corrupt | Baseline ns/read | Candidate ns/read | Change | 95% interval |
| --- | ---: | --- | ---: | ---: | ---: | --- |
| 0 | 0 | false | 3579.5 | 2074.5 | -42.0461% | [-42.5147%, -41.5792%] |
| 0 | 0 | true | 3551.9 | 2069.3 | -41.7391% | [-42.2990%, -41.0968%] |
| 0 | 32 | false | 3633.6 | 2093.7 | -42.3779% | [-44.2263%, -41.0727%] |
| 0 | 32 | true | 3587.2 | 2075.6 | -42.1396% | [-42.4805%, -41.8473%] |
| 0 | 4096 | false | 5434.0 | 3873.1 | -28.7248% | [-29.0197%, -28.4081%] |
| 0 | 4096 | true | 5388.9 | 3855.4 | -28.4554% | [-28.7946%, -28.1629%] |
| 0 | 1048576 | false | 434032.4 | 432910.8 | -0.2584% | [-0.4810%, +0.0726%] |
| 0 | 1048576 | true | 418663.9 | 417108.0 | -0.3716% | [-0.5465%, -0.2148%] |
| 1 | 0 | false | 3566.2 | 2075.6 | -41.7989% | [-42.3577%, -41.0508%] |
| 1 | 0 | true | 3579.7 | 2040.3 | -43.0017% | [-43.6400%, -42.4205%] |
| 1 | 32 | false | 3583.2 | 2097.6 | -41.4615% | [-42.2087%, -40.5704%] |
| 1 | 32 | true | 3599.6 | 2062.0 | -42.7143% | [-43.1396%, -42.2992%] |
| 1 | 4096 | false | 5441.0 | 3896.4 | -28.3879% | [-29.3100%, -27.0058%] |
| 1 | 4096 | true | 5382.2 | 3838.9 | -28.6733% | [-29.1098%, -28.2271%] |
| 1 | 1048576 | false | 433620.5 | 432134.6 | -0.3427% | [-0.5147%, -0.1838%] |
| 1 | 1048576 | true | 418126.4 | 417135.7 | -0.2369% | [-0.3959%, -0.0728%] |

Healthy 0/32-byte reads improve by 41.46–42.38%, and healthy 4 KiB reads by 28.39–28.72% across campaigns. Healthy 1 MiB reads have a first-campaign interval including zero; no repeatable healthy-large-blob speedup is claimed. These are changes relative to the immediately preceding runtime, not additive with earlier optimizations and not a universal Penelope slowdown percentage.

All 1,024 allocation samples agree by revision: baseline `(2 allocations, 0 reallocations, 160 requested bytes)`, candidate `(1, 0, 176)` across every profile. This removes one cache-key allocation but requests sixteen more Rust bytes in total per read. Counters exclude SQLite C allocation and do not measure peak/retained memory or statement allocation footprint.

## Qualification and reproduction

Stable workspace all-target/all-feature qualification (excluding fuzz), SQLite all-target/all-feature qualification on Rust 1.85.0, and strict workspace all-target Clippy passed. SQLite has 35 unit, 61 process and 43 task tests. Existing blob-column, scope-rebinding, repair, schema-change, retirement and caller-buffer checks remain. Formatting, layer checks and the local graph update passed. Debug benchmark timings in qualification logs are not performance evidence.

[Retained evidence](verification-data/2026-10-31-task-blob-snapshot/) includes source snapshots/hashes, the race test, baseline failure, twenty fixed runs, all timing/allocation rows, binary/benchmark hashes, build/qualification logs and deterministic analysis. Run `python3 docs/verification-data/2026-10-31-task-blob-snapshot/verify.py` to check every sample and independently recompute sixteen summaries. The build script temporarily replaces production source and restores it in `finally`; use an idle checkout.
