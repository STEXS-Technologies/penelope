# Reuse SQLite process record encoding storage — 2026-10-31

Fresh atomic process commits now reuse one worker-local encoding buffer across their outcome and action inserts. Previously every row allocated its own buffer and grew it from 128 bytes. SQL, checksums, individual record limits, quota accounting, transaction ordering, and duplicate-commit verification are unchanged.

The measured benefit is lower Rust allocation traffic. Durable latency results are mixed and do not establish a general speedup or a universal application overhead percentage.

Baseline: `0c68fdc754c7be223b0463803029c0aa1a290416`.

## Allocation results

Each value below was identical across all 32 retained baseline observations and all 32 retained candidate observations per profile, including the confirmation comparison. Measurement covers the public commit call and its waiter on all Rust threads, including the database worker. Fixture construction, checksums, replay, and action verification happen outside counting. SQLite's C allocator is not counted. Requested bytes count allocation traffic, including reallocations, rather than live or peak memory.

| Scope identifiers | Outcomes | Actions | Allocations before → after | Reallocations before → after | Requested bytes before → after |
| --- | ---: | ---: | ---: | ---: | ---: |
| Short | 1 | 0 | 14 → 14 | 6 → 6 | 4,450 → 4,450 |
| Short | 1 | 128 | 271 → 143 | 780 → 524 | 307,554 → 192,866 |
| Short | 128 | 0 | 141 → 14 | 266 → 12 | 280,794 → 167,002 |
| Short | 128 | 128 | 398 → 143 | 1,035 → 525 | 585,946 → 357,466 |
| 128 characters | 1 | 0 | 37 → 37 | 13 → 13 | 12,977 → 12,977 |
| 128 characters | 1 | 128 | 1,510 → 1,382 | 979 → 532 | 1,078,897 → 704,113 |
| 128 characters | 128 | 0 | 735 → 608 | 400 → 19 | 620,536 → 376,696 |
| 128 characters | 128 | 128 | 2,208 → 1,953 | 1,361 → 533 | 1,690,552 → 1,071,928 |

For the maximum long-scope batch this removes 255 allocations, 828 reallocations, and 618,624 requested bytes per fresh commit. Single-outcome commits without actions retain their original allocation counts. Duplicate retries return before the new buffer is created.

A standalone guard checks all eight profiles after two warmups, with budgets allowing four allocations, two reallocations, and 1,024 bytes above the observed candidate counts. The old implementation fails the guard on the warmed short-scope, one-outcome, 128-action profile. These are fixture-specific regression budgets, not API promises about allocator internals.

## Correctness and recovery

The new public integration tests cover 1-, 16-, and 128-outcome commits with matching action counts, alternating long and short record identities, and varying digests. Every stored outcome/action is compared byte-for-byte with its own canonical JSON and checked against its durable checksum. Reopening and retrying must return a duplicate receipt without changing any row across ten process tables.

Two additional cases abort the fourth outcome or fourth action insert. Each must roll back all ten tables, then succeed after the trigger is removed, preserve exact record bytes, and return an unchanged duplicate receipt after reopen. A negative control omitting the encoder's buffer clear fails the exact-byte test. The existing encoder unit tests cover exact size boundaries, oversized values, partial serialization failures, and subsequent buffer recovery. Public SQLite configuration requires a record limit of at least 4,096 bytes, so those small synthetic size-boundary checks remain at the encoder level.

## Latency comparisons and rejected alternative

The first candidate reused the complete serialized commit buffer after hashing it. It saved even more allocation traffic but slowed short-scope, 128-outcome, zero-action commits by **2.94%** and **3.36%** in two campaigns; both unadjusted 95% paired-block bootstrap intervals excluded zero. That implementation was rejected.

The retained candidate instead uses a separate buffer beginning at 128 bytes and reuses it across both insertion loops. Two initial campaigns and two confirmation campaigns reuse the same binary pair. There is no profile with supported regressions in more than one retained campaign. There are isolated supported regressions: short-scope single-outcome/zero-action +1.20% in the first campaign, and maximum long-scope batch +4.61% in the second. Other campaigns do not reproduce those intervals. Several large-batch comparisons show supported improvements, but many intervals are broad and cross zero.

The retained implementation is justified by consistent allocation savings and preserved correctness. Timing establishes neither a general speedup nor proof of zero slowdown. All observations, including the noisy campaign and both isolated regressions, are retained.

Each two-campaign comparison has six paired blocks per campaign, alternating ABBA/BAAB executable order, eight profiles, and eight measured fresh commits per profile per executable. Each profile first makes two unreported warmup commits. The ordinary executable has no allocation counter. Allocation traffic is measured in separate executables, with two repeats per binary pair.

Analysis checks fixture checksums, the full observation grid, ordering, and uniqueness. It computes candidate-versus-baseline log-mean changes within each block, then bootstraps the six paired blocks with 20,000 deterministic resamples. Intervals are unadjusted for multiple comparisons. Each reported median is distinct from the paired estimate. A fourth campaign is not treated as an independent compiler rebuild.

Across the rejected comparison, retained comparison, and confirmation: 144 ordinary executable invocations, 12 allocation invocations, 9,216 timed fresh commits, 768 allocation observations, 48 latency summaries, and 24 allocation comparisons. The host is an AMD Ryzen 9 7950X, Linux, Rust 1.99.0, CPU 0. No other project builds or tests ran during timing. Background activity, frequency scaling, and filesystem latency were not controlled. This is a synthetic fresh-commit workload under WAL/FULL durability, not a whole-application benchmark.

## Evidence

[Archived data and verifier](verification-data/2026-10-31-process-record-buffer/) retain compressed raw observations, runtime patches, build/test logs, binary/source hashes, harness hashes, host metadata, and analysis scripts. The confirmation reuses the retained comparison's binaries and patch; it does not claim a separate build. The only harness change after the rejected experiment is a Clippy allowance on the imported fixture module.

Run from this exact checkout:

```sh
python3 docs/verification-data/2026-10-31-process-record-buffer/verify.py
```

Verification reconstructs both candidate patches from the pinned parent in temporary directories, verifies current source and archive hashes, checks negative controls, and recomputes all three summaries from raw data without running builds or benchmarks. Historical build/measure scripts record the experiment procedure and expect their original candidate source/output directory; use fresh output directories for new experiments.

Final qualification commands and statuses are in `qualification.json`: stable workspace tests, Rust 1.85 SQLite tests and all-target checks, strict workspace Clippy, formatting, layer boundaries, and AST graph refresh. No external release plan is copied into this repository.
