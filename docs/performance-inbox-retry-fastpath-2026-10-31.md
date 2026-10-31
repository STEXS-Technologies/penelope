# Read-only exact inbox retries — 2026-10-31

Exact SQLite `Inbox.accept` retries now validate committed input evidence before requesting the WAL writer lock. Previously even a duplicate required an immediate write transaction, returning `Unavailable` when another writer held the lock with a zero busy timeout. Pending and already-consumed inputs can now return their duplicate receipt under that condition.

The read checks the exact process/input key, bounded BLOB types and lengths, and the stored SHA-256 checksum. Canonical bytes use the existing equality shortcut; valid noncanonical JSON still requires semantic equality. Conflicts remain `Conflict`; corrupt evidence remains `Invariant`. Only the statement is cached, never row presence, input bytes, or validation results.

A missing row still requires an immediate transaction. Admission rechecks the row under the writer lock before charging quota or inserting, using the already prepared bytes and key. This preserves single charging when another connection inserts between the initial read and writer admission. Atomic `commit_process` admission and consumption retain their original transaction boundary. Public input validation still precedes worker admission; encoding and key construction now precede writer admission, so their errors can precede writer contention errors.

No public API, SQL schema, SQLite durability setting, quota, or worker queue bound changes. A duplicate observes a committed snapshot: a later writer can change the database after the read, just as it could after the former retry transaction ended. This removes duplicate retries' unnecessary writer-lock dependency; it does not promise success under every possible database lock or error.

## Reliability checks

The new locked validation matrix covers 44 cases: pending/consumed inputs, warmed/reopened stores, canonical/pretty/escaped encodings, conflicting inputs, and malformed, wrongly typed, oversized, short, or incorrect checksum evidence. The original unlocked 44-case matrix also runs unchanged. All ten process/lease-counter table snapshots must remain unchanged.

Four additional cases combine pending/consumed and warmed/reopened stores at full scope quota. Another connection holds `BEGIN IMMEDIATE` and makes an uncommitted corruption. Exact retries must use committed evidence, conflicting inputs must fail, and new inputs or other process/tenant scopes must still require writer admission. After rollback, fresh admission fails with `QuotaExceeded` while the duplicate still succeeds, with all ten tables unchanged.

A deterministic unit test uses SQLite's busy callback and bounded channels, without scheduling sleeps. The initial read sees no row; a concurrent writer then commits that input and fills quota before writer admission completes. The transactional recheck must return the duplicate receipt without a second charge, insertion, or consumption.

Five negative controls restore the old implementation, bypass the canonical checksum check, bypass semantic equality, omit process scope binding, or remove the transactional recheck. Each fails its corresponding test, and candidate sources are restored exactly afterward. Four focused public tests pass again after restoration.

## Retry latency and allocation traffic

The existing ordinary inbox benchmark is unchanged. It measures canonical inputs with fixed short identifiers, separately for pending and consumed duplicates. Each process warms both profiles 64 times and measures 5,000 calls per profile. Two campaigns of eight balanced ABBA/BAAB blocks give 64 processes, 128 timing rows, and **640,000 measured public calls**. These measurements do not establish effects for every input shape or encoding.

The table gives geometric mean call latency and paired candidate change. Intervals use 20,000 deterministic block-bootstrap resamples; 95% intervals are unadjusted for multiple comparisons. Negative means less time.

| Campaign | Input | Baseline µs | Candidate µs | Change | 95% interval |
| --- | --- | --- | --- | --- | --- |
| 0 | Pending | 9.232 | 7.460 | -19.20% | [-19.59%, -18.81%] |
| 0 | Consumed | 9.314 | 7.567 | -18.76% | [-19.15%, -18.44%] |
| 1 | Pending | 9.218 | 7.492 | -18.72% | [-19.05%, -18.31%] |
| 1 | Consumed | 9.244 | 7.534 | -18.49% | [-18.73%, -18.26%] |

All four comparisons support lower retry latency: **18.49–19.20% less time**. Separate allocation probes ran only after ordinary timing: four repeats per variant, 32 measured samples per profile, 512 observations. Every observation in both revisions records **8 Rust allocations, 1 reallocation, and 968 requested bytes**. These counters exclude SQLite's C allocations and measure requested traffic, not peak live memory.

## Fresh durable workload control

Missing inputs now perform an extra initial SELECT. The unchanged complete durable timer workload checks fresh scheduling, timer delivery, payload/input admission, and atomic consumption, followed by untimed durable reopen verification. Both revisions use a Btrfs database directory, CPU 0, work 65,536, no reopen before consumption, batches 1/16/128, and identifier widths 9/128. Two campaigns of six balanced blocks and two alternating direct/managed rounds per process give **288 processes, 576 verified workflows, and 27,840 consumed timers**. Builds, tests, analysis, and allocation probes did not overlap ordinary timing.

| Campaign | Batch | Width | Total-workflow change | 95% interval |
| --- | --- | --- | --- | --- |
| 0 | 1 | 9 | -4.53% | [-12.01%, 3.00%] |
| 0 | 1 | 128 | 1.86% | [-6.76%, 11.19%] |
| 0 | 16 | 9 | -1.74% | [-8.14%, 2.96%] |
| 0 | 16 | 128 | -1.69% | [-7.09%, 2.43%] |
| 0 | 128 | 9 | -1.64% | [-4.30%, 0.97%] |
| 0 | 128 | 128 | -1.29% | [-2.73%, -0.19%] |
| 1 | 1 | 9 | -2.19% | [-18.02%, 17.84%] |
| 1 | 1 | 128 | -2.61% | [-19.91%, 20.67%] |
| 1 | 16 | 9 | 10.92% | [-1.49%, 26.80%] |
| 1 | 16 | 128 | -6.64% | [-15.36%, 3.24%] |
| 1 | 128 | 9 | -7.29% | [-15.36%, 3.70%] |
| 1 | 128 | 128 | -0.22% | [-8.11%, 5.52%] |

One total-workflow comparison supports a gain, eleven are inconclusive, and none supports a regression. Point estimates range from −7.29% to +10.92%; this does not prove zero fresh-admission cost. One scheduling phase, campaign 1/batch 16/width 9, supports a **+19.42% regression [1.28%, 44.54%]**. All delivery, consumption, scheduling, and direct-business controls remain archived, including unfavorable observations. No universal speedup or overall application overhead percentage follows from these results.

## Qualification and reproduction

Stable workspace tests, SQLite/executor/process-runtime tests on Rust 1.85.0, strict workspace Clippy over all targets/features, MSRV all-target SQLite/executor checks, formatting, and layer checks pass. This includes the new lock and insertion-race tests and existing durable partial timer-batch recovery coverage. The graph was updated after exact source restoration.

The baseline process suite passes 74 tests while explicitly skipping the two new lock-behavior tests, which distinguish this change. The candidate passes all 76 process tests plus its insertion-race unit test. Baseline failure against the new lock matrix is separately retained as a negative control.

[Archived evidence](verification-data/2026-10-31-inbox-retry-fastpath/) contains raw observations, exact summaries, source snapshots and hashes, executable hashes, compiler/environment metadata, scripts, and qualification/fault logs. Executables and databases are not committed.

```sh
python3 docs/verification-data/2026-10-31-inbox-retry-fastpath/verify.py
```

The verifier checks file integrity, source provenance, complete measurement grids, recomputed summaries, allocation distributions, qualification, and five rejected controls. For fresh timing, copy the archived scripts and snapshots into a disposable checkout of the pinned parent with the common test files from this commit, and choose a fresh output directory. The builder temporarily replaces the runtime source and restores it in `finally`; do not run project jobs concurrently with source-mutating scripts. Match the recorded compiler, CPU, and Btrfs placement; historical timings are not guaranteed to reproduce exactly.
