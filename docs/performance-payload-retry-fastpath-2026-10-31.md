# Read-only exact payload retries — 2026-10-31

Exact SQLite payload retries can now complete while an unrelated connection holds the WAL writer lock. Previously every `put_payload`, including an exact retry, began an immediate write transaction before checking stored bytes; with an occupied writer and a zero busy timeout, that retry returned `Unavailable`.

The new path reads and validates current committed payload bytes first. An exact match returns the requested digest without opening a write transaction. A mismatch or invalid stored BLOB remains `Invariant`. A missing payload still enters an immediate transaction and **rechecks the row under the writer lock** before quota charging or insertion. Another writer can insert between the initial read and writer admission; the recheck preserves idempotency and single quota charging.

Caller size and SHA-256 checks still precede worker admission. The worker compares bounded BLOB bytes with that already-hashed caller content, under the exact canonical scope/digest key. Only the prepared statement is cached; payload bytes, row presence, and validation answers are never cached. Each read uses current committed evidence. Uncommitted external changes do not replace that evidence. As with any snapshot read, a later writer can change the database after the read has completed; the earlier write-transaction retry had no guarantee against changes after its transaction ended either.

No SQL schema, public API, SQLite durability setting, new-write transaction boundary, worker queue bound, or quota limit changes. Reads may still fail for database errors; this removes the unnecessary requirement for an exact retry to acquire the WAL writer lock, rather than promising contention-free access under every possible lock condition.

## Tests and race coverage

Two new public integration tests cover 24 cases. Twelve exact-retry cases combine 9/128-byte scope identifiers, empty/16-byte/1 MiB payloads, and warmed/reopened stores. An external connection holds `BEGIN IMMEDIATE` and changes bytes without committing. The retry must return the digest from committed evidence, while new payloads and other scopes remain subject to writer admission. Wrong caller digests remain rejected. After rollback, all ten process/lease-counter tables must match the original snapshot. Exact retries also succeed at a fully used quota, while new inserts fail with `QuotaExceeded` after the lock is released.

The other twelve cases publish six committed corruptions after warming the statement, then retry through warmed and reopened stores while an external writer is active. Substituted, empty, oversized, TEXT, INTEGER, and REAL payload evidence must produce `Invariant`, with all ten tables unchanged.

A new unit test deterministically stages an insertion race using SQLite's busy callback and bounded channels, with no scheduling sleeps. A concurrent transaction inserts the same payload and fills the remaining quota. The candidate's initial read sees absence; after writer admission succeeds, the required recheck sees the committed row. The call succeeds and neither payload count nor quota usage changes again.

Existing payload boundary/corruption/quota tests, concurrent quota tests, durable failure/rollback tests, and executor timer-batch recovery tests remain part of qualification.

## Public payload latency

The existing ordinary payload benchmark and fixture are unchanged. It covers four payload sizes and five cases: exact retry, healthy read, missing read, corrupt read, and corrupt put. Each process warms every profile and validates results outside its measured interval. Fixture checks require unchanged stored bytes and usage.

Two campaigns use eight balanced ABBA/BAAB blocks each: 64 processes, 1,280 profile observations, and 1,000,960 measured public calls. The following table gives geometric mean latency in µs, followed by the paired candidate change and its block-bootstrap 95% interval. Negative means less time. Intervals use 20,000 deterministic resamples of eight blocks and are unadjusted for multiple comparisons.

| Payload bytes | Campaign 0: baseline → candidate µs; change [95% interval] | Campaign 1: baseline → candidate µs; change [95% interval] |
| --- | --- | --- |
| 0 | 10.159 → 7.238; -28.75% [-28.96, -28.58] | 10.380 → 7.295; -29.72% [-31.63, -28.53] |
| 32 | 10.212 → 7.176; -29.72% [-30.21, -29.26] | 10.262 → 7.362; -28.26% [-29.34, -26.46] |
| 4096 | 12.116 → 9.129; -24.65% [-25.94, -22.60] | 12.161 → 9.050; -25.58% [-26.29, -24.96] |
| 1048576 | 455.228 → 453.390; -0.40% [-1.06, 0.39] | 453.747 → 450.289; -0.76% [-1.38, -0.16] |

All six 0–4 KiB retry comparisons are supported gains, approximately 25–30% less time. At 1 MiB, one campaign is inconclusive and one supports a small gain; caller hashing/copying remains part of the public call. There are no supported exact-retry regressions. All eight corrupt-put comparisons are supported gains, with point estimates from −30.07% to −0.85%.

The unchanged read controls are not uniformly faster: the 32-byte healthy read in campaign 1 has a supported +0.414% regression [0.084%, 0.771%]. Other healthy/missing/corrupt-read controls are inconclusive except one supported corrupt-read gain. All profiles and unfavorable results are retained; this is not a claim that every payload path improved.

## Fresh durable workflow control

Because missing payloads now perform an additional initial SELECT, the candidate was also compared with the parent using the unchanged complete durable timer workload. It includes fresh result/timer payload writes and their input consumption, not only duplicate retries. Both revisions use the same Btrfs temporary-database directory, one pinned CPU, 65,536 business-work units, no reopen before consumption, batches 1/16/128, and widths 9/128. Every observation still performs final durable reopen verification and idempotent commit retries outside timing.

Two campaigns use six balanced ABBA/BAAB blocks, six configurations, and two alternating direct/managed rounds per process: **288 processes, 576 verified workflows, 27,840 consumed timers**. Builds, tests, analysis, and allocation probes did not overlap ordinary timing.

| Campaign | Batch | Width | Paired total-workflow change | Bootstrap 95% interval |
| --- | --- | --- | --- | --- |
| 0 | 1 | 9 | 1.31% | [-9.62%, 15.34%] |
| 0 | 1 | 128 | -0.36% | [-4.06%, 3.68%] |
| 0 | 16 | 9 | 2.47% | [-3.05%, 8.72%] |
| 0 | 16 | 128 | 1.46% | [-2.97%, 6.41%] |
| 0 | 128 | 9 | -1.54% | [-8.97%, 3.86%] |
| 0 | 128 | 128 | -7.18% | [-13.10%, -1.72%] |
| 1 | 1 | 9 | -18.08% | [-28.22%, -2.75%] |
| 1 | 1 | 128 | 7.60% | [-12.39%, 31.60%] |
| 1 | 16 | 9 | -2.29% | [-11.94%, 11.62%] |
| 1 | 16 | 128 | -2.21% | [-12.02%, 6.44%] |
| 1 | 128 | 9 | 0.07% | [-4.57%, 5.41%] |
| 1 | 128 | 128 | 1.12% | [-6.75%, 9.81%] |

Two total-workflow comparisons are supported gains and ten are inconclusive; none is a supported total regression. This does not establish zero cost for fresh writes. Phase results include two supported regressions: schedule +8.39% and consumption +4.06%, both for batch 16/width 128 in campaign 1. Two direct-business CPU controls also regress by +0.29% and +0.76%. Every phase/control is archived. No overall application slowdown percentage or universal improvement is inferred from this control workload.

## Rust allocation traffic

Separate instrumented processes ran after all ordinary timing: four repeats per revision, 16 measured samples after warmup, 20 profiles, **2,560 observations**. Exact retries allocate one additional Rust object and request 80 more bytes for the cached statement handle; reallocations remain zero. For a 32-byte retry, median counts change from 7 allocations / 578 requested bytes to 8 / 658. Empty retries change from 6 / 546 to 7 / 626; 4 KiB and 1 MiB retries have the same +1 allocation/+80-byte delta. Corrupt puts have the same delta. Read controls retain their allocation medians. This tradeoff is accepted for lower retry latency and removal of writer-lock dependence. These counters exclude SQLite's C allocations and measure requested traffic, not peak live memory.

## Qualification and evidence

Stable workspace tests, SQLite/executor/process-runtime tests on Rust 1.85.0, strict workspace Clippy over all targets/features, MSRV all-target checks, formatting, and layer checks passed. The graph was updated. Focused tests confirm the new lock behavior and insertion race on the supported compilers. Baseline process qualification explicitly skips the two new lock-behavior tests, whose purpose is to distinguish the new behavior; the existing process suite passes in both revisions. The new race unit test is selected for candidate qualification.

Negative controls restore the parent implementation, bypass stored byte equality, omit scope binding, or remove the transactional insertion recheck. Each must fail its corresponding new test, and candidate sources must be restored exactly afterward. These checks and final qualification are recorded in the archive.

[Archived evidence](verification-data/2026-10-31-payload-retry-fastpath/) includes raw timing/allocation observations, all paired summaries, source/binary/environment hashes, both runtime source snapshots, build/measurement/analysis scripts, tests, and fault/qualification logs. Executables are identified by hash rather than committed.

```sh
python3 docs/verification-data/2026-10-31-payload-retry-fastpath/verify.py
```

The verifier checks provenance and complete observation grids, independently recomputes both summaries and allocation distributions, and checks qualification/fault records. To repeat timing, copy the archived scripts and source snapshots into a disposable checkout of the pinned parent, with the common test files from this commit, and use a fresh output directory. The builder temporarily switches the runtime file and restores the candidate in `finally`; do not run builds/tests concurrently with it. Match the recorded compiler and Btrfs placement before comparing fresh observations; exact historical timings are not guaranteed.
