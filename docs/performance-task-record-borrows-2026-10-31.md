# Borrowed task-record decoding — 2026-10-31

Task journal reads now decode the record while SQLite's row bytes remain borrowed, returning the same owned, fixed-size `TaskRecord`. This removes the temporary record and checksum vectors. BLOB type/length bounds, checksum verification, scope/sequence/version validation, retired-scope rejection, and the head/tail consistency check remain in place. Decode errors retain `TaskPortError::Corrupt`; SQL errors retain their existing classification. No schema change is required.

Baseline: `523eab8a4eb8539267d25573b56f4f77b93d40c7`. The code commit is pinned in the accompanying evidence metadata.

The ordinary fixture commits an input intent through the public task port. The padded fixture externally replaces that record with the same valid JSON plus whitespace up to 4,096 bytes, recomputing its record checksum. It is a synthetic decoding boundary, not a second production workflow; its head digest/usage are not reconstructed. The end-of-log control requests sequence 1 after the one-record history. Returned records are checked before and after each timed sample, outside the timer; errors fail the process. Fixtures use fresh local WAL/FULL databases, 100 warmup reads, and 1,000 timed reads per profile.

A 24-pair alternating AB/BA campaign ran 96 processes: ordinary-allocator latency and a separate allocation-traffic executable for each variant. All ran on CPU 0, AMD Ryzen 9 7950X, Linux 7.2.9-100.fc43.x86_64, release Rust 1.99.0. No agent-owned builds, tests, or graph updates ran during measurement. Each allocation profile has 384 samples per variant. The allocator instrumentation forwards to System and counts Rust allocations/reallocations and cumulative requested bytes; it excludes SQLite C allocation and does not measure peak memory.

| Read profile | Rust allocations before → after | Requested bytes before → after | Mean paired latency change | Paired bootstrap 95% interval |
|---|---:|---:|---:|---:|
| Ordinary intent (1,048-byte JSON) | 3 → 1 | 1,272 → 192 | +0.398% | −4.600% to +6.457% |
| Padded intent (4,096-byte JSON) | 3 → 1 | 4,320 → 192 | −0.488% | −3.740% to +1.478% |
| End-of-log | 1 → 1 | 192 → 192 | −2.033% | −6.467% to +1.338% |

Reallocations were zero for both variants in all profiles. Two allocations and exactly `record_length + 32` requested bytes are removed per successful record read. The timing intervals all include zero: **this campaign establishes lower allocation traffic, not a latency speedup or a new application-wide overhead percentage**. Percentages are the mean of 24 paired relative changes; confidence bounds resample those pairs 20,000 times with a fixed seed. Ratios of aggregate means are a different statistic.

Regression fixtures cover 21 combinations: original/at-limit/oversized/empty/malformed/null/trailing/invalid-UTF-8 records, wrong embedded scope/sequence/version, TEXT/INTEGER/REAL records, and TEXT/INTEGER/REAL/empty/short/wrong/oversized checksums. Each verifies the result, unchanged durable rows, held owned results, repaired reads on the same connection, and reopened reads. CHECK constraints are disabled only while injecting persisted damage. Both baseline and candidate pass the fixture. Twenty isolated final runs (ten stable, ten MSRV 1.85.0) cover 420 cases.

Raw campaign rows, binaries' hashes, source pins, build/reproduction scripts, exact summaries and validation logs are archived in [verification data](verification-data/2026-10-31-task-record-borrows/campaign.json). To rebuild and repeat, copy the data directory to `target/task-read-comparison`, run `build.py` from the repository root, then run `compare.py` and `analyze.py` from that directory. The build script temporarily installs the archived baseline production source and restores the current candidate in `finally`; it expects the benchmark fixtures from this code commit. Do not run other local jobs during the comparison. Neither qualification smoke benchmark output nor the old application overhead fixture supplies comparative evidence for this change.

The final workspace all-target/all-feature tests excluding `penelope-fuzz`, MSRV SQLite all-target/all-feature tests, strict workspace Clippy, formatting, layer boundaries, and `graphify update .` passed. SQLite totals were 24 unit, 51 process, and 29 task tests. The archive verifier recomputes all three timing summaries and checks source/data hashes, allocation sample consistency, and all 20 repeated regression runs.
