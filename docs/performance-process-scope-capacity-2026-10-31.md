# Process scope encoding capacity, 2026-10-31

`ProcessScope::canonical_bytes` reserves the exact bounded encoding length before writing its unchanged four length-prefixed identifiers and definition digest. This removes repeated growth of the identity vector without changing canonical bytes, validation or public APIs. Baseline: `96c43d49ab9d78a71ac5693964294a6ac5681071`.

A 10,005-case legacy-encoding oracle covers varied identifier lengths, all digest byte values and explicit minimum, inline-storage boundaries and maximum lengths. The isolated allocation test requires one allocation for short and maximum-length scopes. The previous allocator's default reallocation implementation is counted as an allocation by that test.

The unchanged public process-payload fixture measures effects through real worker dispatch and SQLite. Two campaigns of eight balanced ABBA/BAAB blocks, four processes per block, produced 64 processes, 1,280 rows and 1,000,960 measured operations across five cases and four sizes. Warmups/iterations are 100/1,000 except 1 MiB profiles at 32/128. All results, storage bytes, corruption rejection, repair and usage checks remain enabled. CPU 0, Ryzen 7950X, Linux 7.2.9, project NVMe btrfs storage; no project builds/tests/analysis overlapped ordinary timing. Background load and frequency were uncontrolled.

Healthy read results (negative means faster):

| Campaign | Bytes | Change | Block-bootstrap 95% interval |
| --- | ---: | ---: | --- |
| 1 | 0 | -1.3336% | [-1.6981%, -0.9463%] |
| 1 | 32 | -0.8051% | [-1.0914%, -0.4868%] |
| 1 | 4096 | -0.8903% | [-1.4704%, -0.1534%] |
| 1 | 1048576 | -0.0932% | [-0.4840%, +0.2449%] |
| 2 | 0 | -1.4425% | [-1.8086%, -1.0588%] |
| 2 | 32 | -0.5624% | [-0.9828%, -0.0768%] |
| 2 | 4096 | -0.8821% | [-1.3272%, -0.4183%] |
| 2 | 1048576 | +0.0974% | [-0.1408%, +0.3391%] |

All small read, missing and corrupt-read profiles improve with intervals below zero in both campaigns. There is no repeatable 1 MiB read-path benefit; their intervals include zero. Full unrounded results for all 40 profiles, including put paths, are retained in summary.json. Intervals use 20,000 deterministic block bootstrap samples per profile, without multiple-comparison adjustment. No application-wide overhead claim follows from these measurements.

Four separate allocation-probe processes per revision, 16 samples for each of 20 profiles, yield 2,560 samples total. Every measured public operation removes four Rust reallocations and 174 requested bytes, with allocation counts unchanged. For example, a 32-byte healthy read changes from (8 allocations, 4 reallocations, 704 requested bytes) to (8, 0, 530). This fixture uses short identifiers; traffic saved for other identifier lengths may differ. SQLite C allocations are excluded. Requested allocation traffic is not live or peak memory.

Both revisions pass 61 process tests. The candidate passes workspace all-target/all-feature tests excluding fuzz, domain and SQLite all-target/all-feature tests on Rust 1.85.0, strict Clippy, formatting and layer checks. SQLite has 35 unit, 61 process and 45 task tests; the new wire-compatibility oracle and isolated allocation assertions pass on both toolchains.

[Evidence and reproduction scripts](verification-data/2026-10-31-process-scope-capacity/) retain raw data, all summaries, snapshots, hashes and qualification logs. `python3 verify.py` checks the evidence and recomputes timing summaries. In a disposable baseline checkout, copy the evidence directory, run its build.py from repository root, then timing.py and summarize.py. Build temporarily replaces the domain implementation and leaves the candidate afterward. Do not run it in an active worktree. Matching binary hashes requires the same toolchain/build environment. Debug qualification benchmark times are not performance evidence.
