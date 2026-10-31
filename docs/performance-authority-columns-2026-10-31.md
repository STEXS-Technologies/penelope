# Generation metadata validation and cached authorization head — 2026-10-31

Baseline: `cd2e0c6` (full commit and final code/source/binary pins accompany the evidence).

Reservation retries previously parsed an execution identity into a 32-byte digest only for state 0. Short BLOB identities in states 1 or 2 were rejected as `Conflict`, while the same malformed values in state 0 returned `Corrupt`. The new 27-case regression matrix reproduces four such classification differences on the baseline. They were already rejected: this is an improvement to corruption reporting, not evidence of a false reservation or authority bypass.

Optional execution identities now decode directly into fixed-size `Digest` values for all states. NULL handling and valid identity/fence behavior are preserved. Authorization compares the head's sequence, borrowed digest and byte count against the reconstructed projection without creating a digest vector. The head-verification SQL uses a cached prepared statement with fresh bindings/results. Explicit head rebuilding keeps its existing decoder so it can still repair derived digest metadata. Confirmation binds an optional borrowed digest rather than allocating a vector; its SQL equality and row-count rules are unchanged. No schema migration or durability-mode change is introduced.

The benchmark reuses public exact `reserve_start` retries on 2, 16 and 128 publicly committed records (intent, start, repeated checkpoints), with one existing reservation and five-byte input/checkpoint blobs. Each retry fully reconstructs authorized history and returns `AlreadyReserved`. Fresh WAL/FULL fixture setup is excluded, as are post-sample checks. It does not time initial admission, fence confirmation, or an application workflow.

The initial allocation-only candidate removed both 32-byte temporary vectors. A 24-pair AB/BA campaign ran 96 processes, with ordinary latency and separate allocation traffic probes. Timing was inconclusive:

| Retained records | Uncached candidate vs baseline | Bootstrap 95% interval |
|---:|---:|---:|
| 2 | +1.566% | -0.336% to +3.570% |
| 16 | -0.263% | -1.310% to +0.713% |
| 128 | +0.469% | -1.214% to +2.727% |

The final candidate also caches the head-verification statement. A second, balanced three-variant campaign used all six variant orders four times (24 blocks, 144 processes). The baseline and uncached executable hashes match those used in the initial campaign. All observations are preserved, including the uncached candidate's slower results in this second campaign:

| Records | Uncached vs baseline (95% interval) | Final cached vs baseline (95% interval) | Final cached vs uncached (95% interval) |
|---:|---:|---:|---:|
| 2 | +5.331% [+0.041%, +12.956%] | -4.745% [-5.792%, -3.564%] | -8.016% [-12.610%, -4.613%] |
| 16 | +4.639% [+0.882%, +10.743%] | -0.596% [-1.695%, +0.963%] | -3.928% [-8.110%, -1.065%] |
| 128 | +1.675% [+0.080%, +3.567%] | +0.349% [-0.985%, +1.936%] | -1.213% [-2.773%, +0.028%] |

The final two-record retry improves by 4.745% (21.722 → 20.684 µs mean elapsed per retry). The final 16- and 128-record comparisons with baseline are inconclusive. The 128-record point estimate is +0.349%; its mean elapsed changes from 468.545 to 469.932 µs. These results do not establish a universal speedup, absence of slowdown on other workloads, or a new application-wide overhead percentage.

There is an explicit allocation tradeoff:

| Variant | Rust allocations per retry | Reallocations | Cumulative requested bytes |
|---|---:|---:|---:|
| Baseline | 2 | 0 | 64 |
| Uncached fixed metadata | 0 | 0 | 0 |
| Final cached head query | 1 | 0 | 96 |

Every measured allocation sample agrees across all three history sizes. The final variant removes both metadata vectors but the rusqlite 0.40.2 cache-key path creates an `Arc<str>` for the SQL on retrieval; local `cache.rs` and `raw_statement.rs` inspection supports that attribution. The final retry has one fewer allocation but requests 32 more bytes than baseline. We retain it for the measured short-history gain and consistent malformed-metadata classification. Counts exclude SQLite C allocations, setup, caller-owned state and peak memory; the final operation is not Rust-allocation-free.

Both campaigns ran on CPU 0 of an AMD Ryzen 9 7950X, Linux 7.2.9-100.fc43.x86_64, release Rust 1.99.0, without other agent-owned builds/tests/graph jobs during timing. Each profile warms 16 calls and times 250 retries; separate probes count 16 allocation samples. Across both campaigns there are 240 processes, 360 latency observations (90,000 timed retries) and 5,760 allocation samples. Percentages average relative changes within a pair/block; 95% bounds resample those 24 pairs/blocks 20,000 times with a fixed seed. Ratios of aggregate means and arithmetic sums of earlier campaign gains are different statistics.

The regression matrix covers matching/different/NULL identities and empty, short, oversized, TEXT, INTEGER and REAL values in each durable state 0/1/2. Fault injection temporarily bypasses CHECK constraints to simulate persisted damage. It checks reservation classification, unchanged rows on rejection, fence behavior, mismatching confirmation rejection, idempotent confirmation, repair and reopen. A second fixture alternates two healthy scopes, creates/drops an index to invalidate cached statements, corrupts and repairs one head's byte count, and verifies that the other scope remains usable. Twenty isolated final processes (ten stable and ten MSRV) exercise 540 matrix cases plus cache rebinding/repair/DDL controls.

[Evidence](verification-data/2026-10-31-authority-columns/final-campaign.json) includes both complete campaigns, source/binary hashes, exact summaries, failing baseline and qualification logs. To rebuild, copy the data directory to `target/authority-columns-comparison`, check out the final code commit, and run `build-three.py` from the repository root. It temporarily installs each archived production variant and restores the final source in `finally`. Run `compare-three.py` and `analyze-three.py` for the balanced campaign, or `compare.py` and `analyze.py` for the initial two-variant comparison after mapping the uncached binaries to its `candidate-*` names. Avoid other local jobs during timing. Broad qualification benchmark output is smoke validation only.

Final qualification passed: workspace all-target/all-feature tests excluding `penelope-fuzz`, SQLite MSRV all-target/all-feature tests (24 unit, 51 process, 34 task), strict workspace Clippy, formatting, layer boundaries and graph update. Seven optional conformance tests passed against pinned StateChronicle `ae3d94bc22c7835eaa3fdcd766e3c117929c12bb`, including SIGKILL recovery. The archive verifier recomputes all 12 latency summaries, checks exact allocation traffic and binary reuse, validates source/data hashes and confirms the 20 repeated matrix/cache test processes.
