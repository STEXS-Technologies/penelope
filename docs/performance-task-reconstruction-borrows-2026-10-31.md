# Borrowed task-history reconstruction — 2026-10-31

Reconstructing task history now decodes bounded record and checksum columns directly from SQLite's borrowed row bytes. After `decode` verifies SHA-256 against those exact bytes, reconstruction copies the verified 32-byte checksum into its last-record digest instead of hashing the record again. Record order, semantic projection, referenced-blob verification, byte accounting and head authorization remain unchanged. This removes two temporary Rust allocations and a redundant record hash per retained record. Blob reads still use their existing owned buffers.

Baseline: `b82cb2130048062677730a3e7272226c86505af4`. The final code commit and source/binary hashes are pinned in the accompanying evidence.

The benchmark uses public exact `reserve_start` retries on a previously reserved execution. Each retry performs full authorized reconstruction and returns `AlreadyReserved`; it does not create a new reservation. Fresh WAL/FULL fixtures contain 2, 16 or 128 publicly committed records: intent, start, then repeated checkpoints. Input and checkpoint payloads are each five bytes; the fixture deliberately reuses the same checkpoint blob. The 256-record budget retains a final-outcome slot and keeps the 128-record task valid and running. Setup and writes are excluded from timing. Return values, retained record count and single reservation count are checked. This measures authorization retries with full reconstruction, not application-wide overhead, initial reservation, retirement, or every workload using reconstruction.

Twenty-four alternating AB/BA pairs ran 96 processes on CPU 0 of an AMD Ryzen 9 7950X, Linux 7.2.9-100.fc43.x86_64, release Rust 1.99.0. Each variant ran ordinary-allocator latency and a separate allocation-traffic executable. No agent-owned builds, tests or graph updates ran during the comparison. Profiles use 16 warmup calls, then 250 timed retries or 16 allocation samples. All completed observations are retained: 144 latency samples (36,000 timed retries) and 2,304 allocation samples.

| Retained records | Mean paired latency change | Paired bootstrap 95% interval | Mean baseline → candidate per retry |
|---:|---:|---:|---:|
| 2 | −3.067% | −3.846% to −2.205% | 22.538 → 21.843 µs |
| 16 | −6.610% | −8.363% to −5.367% | 77.800 → 72.538 µs |
| 128 | −5.750% | −10.008% to −0.303% | 520.746 → 488.858 µs |

Percentages average each pair's relative change; they are not ratios of aggregate means. Bounds resample the 24 pairs 20,000 times with a fixed seed. The 128-record interval is wider and its upper bound is close to zero; the exact samples are preserved.

| Retained records | Rust allocations before → after | Requested bytes before → after | Removed bytes per retry |
|---:|---:|---:|---:|
| 2 | 7 → 3 | 1,681 → 69 | 1,612 |
| 16 | 49 → 17 | 11,479 → 139 | 11,340 |
| 128 | 385 → 129 | 90,047 → 699 | 89,348 |

All 384 samples per profile and variant agree. Reallocations are zero throughout. Removed traffic is exactly two allocations per record and the sum of each encoded record length plus its 32-byte checksum. The allocator measures cumulative requested Rust allocation bytes, excluding SQLite C allocation and peak memory. These results do not imply that reconstruction is allocation-free or that the timing gain applies to ordinary cached appends.

The existing 21-case record-column matrix now also exercises reconstruction through `rebuild_head`. It covers original and 4 KiB whitespace-padded records, oversized/malformed/semantically wrong records and invalid checksum values/types. Valid rebuilds must produce the exact raw-byte digest and record-plus-blob byte count; corrupt inputs must leave durable rows unchanged. Repair must permit rebuilding, reading held owned records, appending a valid start from the rebuilt cache, and reopening. The padded case specifically checks that the last digest identifies the retained encoding rather than reserialized parsed data. Both baseline and candidate pass. Twenty isolated final runs, ten per compiler, cover 420 matrix cases (40 valid, 380 corrupt), their read/reconstruction assertions, and repair/append/reopen controls.

During preflight, the initial 128-record fixture correctly hit the protocol's reserved-final-record quota when its budget was also 128. The fixture budget was raised to 256, both variants were rebuilt and all profiles passed preflight before the complete campaign. That failure log and successful final preflights are archived; no completed comparison pairs were discarded.

[Raw evidence and metadata](verification-data/2026-10-31-task-reconstruction-borrows/campaign.json) include all samples, exact summaries, source/binary pins and build/comparison/analyzer scripts. To reproduce, copy the data directory to `target/task-reconstruction-comparison`, check out the pinned code commit, run `build.py` from the repository root, then run `compare.py` and `analyze.py` there. The builder temporarily installs the archived baseline production source and restores the candidate in `finally`. Avoid other local jobs during comparison. Qualification benchmark output is smoke validation only.

Final qualification passed: workspace all-target/all-feature tests excluding `penelope-fuzz`, SQLite MSRV all-target/all-feature tests (24 unit, 51 process, 30 task), strict workspace Clippy, formatting, layer boundaries and graph update. The evidence verifier recomputes all latency summaries, validates exact allocation deltas, confirms all 20 repeated matrix runs and checks source/archive hashes.
