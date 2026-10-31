# Registry fixed-buffer latency — 2026-10-31

The fixed-buffer registry change reduces repeated public-fence latency by a
paired median **2.20%** in this local benchmark, with a descriptive bootstrap
95% interval of **−2.72% to −1.68%**. The case with a different
worker CPU history shows a similar repeated-fence reduction. Same-CPU live
worker fencing improves by **3.63%**, interval **−4.86% to −1.15%**. Other live
and exited fence intervals include zero. Full-workflow completion intervals
also include zero, so no overall workflow speedup is established.

These percentages compare two versions of the protected runtime. They are
not Penelope-versus-direct-execution overhead measurements and do not replace
earlier source-pinned application overhead results.

## Source and method

Baseline: `a0cd454f10a25e85675e44831a658e38354e02a0`.
Candidate: `cd099118f55bf2c8d38b6d0d5becce6e4daa2bb4` (code commit `236d897`).
Both release binaries were built from the candidate checkout, substituting
only registry.rs from baseline for the baseline build. The only other changed
Rust files between these commits are tests/allocation instrumentation, outside
the benchmark builds. The benchmark and worker fixture sources are identical.

The earlier [allocation qualification](performance-registry-buffers-2026-10-31.md)
measured 29 → 24 Rust allocations per repeated public fence on Rust 1.99.0 and
1.85.0. This campaign measures wall-clock latency independently on Rust 1.99.0.
It retains the new descriptor regular-file checks in both variants.

There are 20 alternating baseline/candidate pairs. Each process measures 32
samples per fence profile, or 16 samples per dispatch profile. One full process
per binary/case is warmup and excluded from summaries. The campaign contains
126 processes and 9,450 worker fixtures, including warmups.

The parent is pinned to CPU 0. Ordinary workers inherit CPU 0. The other-CPU
campaign pins its own live/repeated worker to CPU 1 before timing; exited
controls remain on CPU 0. Repeated fences time an already fenced generation,
so no worker is running during that timed call. CPU topology, toolchain and
binary hashes are retained in metadata. No own builds, tests or graph updates
ran during the timed campaign. Host load, frequency and caches were uncontrolled.

The public fence includes registry locking, manifest validation, WAL/FULL
authority begin/confirm and worker termination/reaping where applicable. Worker
startup/setup and post-fence safety assertions are outside that timer. Dispatch
profiles separately include protected execution, checkpoint acknowledgement,
durable completion and fencing; their timing boundaries are defined by the
retained benchmark source. Every fixture validates its expected outcome.

## Results

| Profile | Baseline median ms | Fixed-buffer median ms | Paired change | Bootstrap 95% interval |
|---|---:|---:|---:|---:|
| Live worker, same CPU | 1.2885 | 1.2371 | -3.63% | [-4.86%, -1.15%] |
| Exited worker | 1.2019 | 1.1958 | -0.92% | [-2.97%, +0.39%] |
| Repeated fence | 0.0866 | 0.0850 | -2.20% | [-2.72%, -1.68%] |
| Live worker, other CPU | 1.2522 | 1.2310 | -1.06% | [-4.75%, +0.92%] |
| Exited control, other-CPU campaign | 1.1919 | 1.1831 | -0.63% | [-2.66%, +1.15%] |
| Repeated fence, other-CPU history | 0.0857 | 0.0836 | -2.26% | [-2.84%, -0.71%] |
| Full workflow, fresh runtime/store | 5.9125 | 5.8927 | -0.28% | [-2.84%, +1.87%] |
| Full workflow, reused runtime/store | 5.6285 | 5.5952 | -1.16% | [-3.08%, +2.14%] |

Each baseline/candidate entry is the median of process medians. Paired change
is the median of the 20 per-pair percentage changes, so it need not equal the
percentage difference between the displayed medians. Negative values are faster.
The intervals use 10,000 paired bootstrap resamples, seed 3030. They are
individual descriptive intervals, without correction for multiple comparisons.

All 16 metrics and every paired change are retained, including setup, intent
commit, start-return and checkpoint acknowledgement. The near-zero reused-store
setup control is timer-scale and its percentage is not a useful speed measure.
No profile has a demonstrated slowdown by these individual intervals, but this
campaign cannot exclude smaller regressions or establish a universal benefit.
The full workflow's fresh and reused completion changes are −0.28% and −1.16%,
respectively, with both intervals including zero.

## Evidence and reproduction

[Evidence manifest](performance-data/2026-10-31-registry-latency/evidence.json)
retains source pins, source hashes and artifact hashes. The directory contains
all process stdout/stderr and raw nanosecond samples, build logs, build and
comparison scripts, deterministic summaries and binary hashes. Gzip timestamps
are fixed. `verify.py` independently reparses process output, checks alternating
order and sample counts, recomputes every summary and validates artifact/build
hashes. It verifies all 126 processes and 9,450 fixtures.

Verification command:

```sh
python3 docs/performance-data/2026-10-31-registry-latency/verify.py
```

The executed build script expects the earlier comparison runner in target/.
For reproduction in a disposable checkout of the candidate, create
`target/qualification-temp`, copy this directory's `compare.py` to
`target/compare-fence-wait.py`, and run this directory's `build.py`. It builds
both variants and restores registry.rs in a finally block. Then run:

```sh
python3 target/compare-registry-latency.py --output target/registry-latency-reproduction --pairs 20 --samples 32 --dispatch-samples 16
```

The script's fixed CPU and TMPDIR settings reflect the qualification host;
adapt them for another host and record the change. Do not compile or test during
timing. Different machines/filesystems need their own measurements.

No production code changed for this follow-up measurement. The source's full
workspace qualification on both Rust toolchains and strict Clippy are retained
in the allocation report. Its GitHub CI run 41156849684 completed successfully;
all seven job results are retained in `source-ci.json`.
