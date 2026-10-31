# Rejected SQLite task-commit statement caching, 2026-10-31

Caching three statements in checkpoint commits produced no repeatable latency improvement and added three Rust allocations requesting 320 bytes per commit. The candidate was rejected and the production source restored byte-for-byte to baseline `fbba2baee58bfd09430240c24d36efd4e3b09c53`.

The candidate cached blob INSERT, duplicate-blob SELECT, and conditional head UPDATE statements. Transaction boundaries, exact duplicate-byte comparisons, and affected-row checks were unchanged. This experiment does not invalidate earlier measured benefits from caching read statements.

## Measurement

The unchanged `task_blob_commits` release benchmark commits initial intent and Started records, then times 128 public checkpoint commits reusing one blob. It checks each receipt and reconstructs the final head, with 130 records and one blob. Four blob sizes were tested. There is no additional warmup beyond the two setup commits and no external writer. These results cover this durable checkpoint path, not universal Penelope overhead, initial commits, new blobs, or no-blob commits.

Two campaigns each used 12 balanced ABBA/BAAB blocks of four processes: 96 processes, 384 timing rows, and 49,152 measured checkpoint commits. Caller and worker ran on CPU 0 on a Ryzen 7950X, Linux 7.2.9; temporary databases used project NVMe btrfs storage. No project builds or tests ran during timing. Background system load and CPU frequency were uncontrolled.

Changes are geometric candidate/baseline ratios within blocks. Positive means slower. Intervals use 20,000 deterministic block bootstrap samples. All eight intervals include zero: neither a repeatable improvement nor a reliable regression was established.

| Campaign | Blob bytes | Latency change | Bootstrap 95% interval |
| --- | ---: | ---: | --- |
| 1 | 0 | +1.3365% | [-4.9757%, +10.1326%] |
| 1 | 32 | -0.3244% | [-7.7187%, +8.2770%] |
| 1 | 4096 | -0.4050% | [-6.3999%, +6.1841%] |
| 1 | 1048576 | +1.0914% | [-5.8548%, +10.9603%] |
| 2 | 0 | +0.9418% | [-6.4188%, +9.5622%] |
| 2 | 32 | +3.3717% | [-2.3203%, +10.2931%] |
| 2 | 4096 | +5.4038% | [-3.2606%, +17.0289%] |
| 2 | 1048576 | +2.4483% | [-0.9505%, +6.5308%] |

Separate allocation probes ran four processes per revision, each with four sizes and 128 checkpoint commits: 2,048 measured samples per revision. Every baseline sample reported 4 allocations, 3 reallocations and 2,232 requested bytes; every candidate sample reported 7 allocations, 3 reallocations and 2,552 requested bytes. Each process also emits four profile metadata rows, which are excluded from these counts. Requested bytes measure Rust allocator traffic, including reallocations, not live or peak memory. SQLite C allocations are excluded.

## Correctness and disposition

Both revisions passed all 44 task integration tests before measurement. The candidate additionally passed workspace all-target/all-feature tests excluding the fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting and layer checks. These include SQLite's 35 unit, 61 process and 44 task tests. After restoration, the 44 task tests were repeated. No runtime change is retained.

## Evidence and reproduction

[Evidence directory](verification-data/2026-10-31-task-commit-cache-rejected/) contains raw timing and allocation rows, summaries, qualification logs, source snapshots, probe, allocator and scripts. `source-hashes.json` identifies the unchanged ordinary benchmark and restored production source; `metadata.json` records measured binary hashes. Binaries are build artifacts and are not committed.

Run `python3 verify.py` in the evidence directory to validate counts and recompute the timing summary. To rebuild the experiment, use a disposable checkout of the baseline revision, copy this evidence directory into it, and run `python3 <evidence-directory>/build.py` from the repository root, then `timing.py` and `summarize.py`. The builder temporarily changes the manifest and source and restores the candidate source afterward; do not run it in an active worktree. It requires the same Rust toolchain for comparable binary hashes. Qualification logs describe the candidate; restored-tests.txt describes the restored source.
