# Reused task blob commits — 2026-10-31

A task commit that reuses an existing blob now hashes the caller buffer once and
compares the stored BLOB directly with that verified buffer. Previously it also
hashed the stored bytes before comparing them. Equality proves both the stored
length and digest after caller verification. SQL type and size bounds still run;
read and reconstruction continue to hash independently because they have no
verified caller buffer.

Baseline: `9a1e80338b581600691f41ef6a7cb39e4f2255c9`.
Candidate: `51b67d9` (full commit and source hashes in the evidence manifest).

The change preserves transaction order, quota accounting, WAL/FULL configuration,
and rollback behavior. It does not address external deletion of a previously
referenced blob while a writer retains its projection cache. That separate cache
invalidation question remains under investigation.

## Measurement

The new `task_blob_commits` benchmark creates a fresh public task journal, commits
an input and a start, then times 128 public checkpoint commits reusing that input
blob. Record proposal and projection updates are outside each timed interval;
transaction work, encoding, validation, blob lookup, record/head writes and commit
are included. All 130 records and the single retained blob are checked afterward,
and a full reconstruction must succeed. Setup and reconstruction are not timed.
No weakened synchronization settings or allocator instrumentation are used.

There are 24 paired runs, alternating AB/BA order, 48 processes and 192 size
observations: 24,576 timed commits total. Processes run sequentially on CPU 0,
AMD Ryzen 9 7950X, Linux 7.2.9, release Rust 1.99.0. No other agent-owned build,
test, or graph jobs ran during measurement. Ordinary system noise remains.

The reported percentage is the mean paired `(candidate / baseline - 1) * 100`.
Intervals use 20,000 deterministic paired bootstrap resamples, percentile 95%.
All pairs are retained. Means in the timing columns are arithmetic means.

| Reused blob | Baseline ns/commit | Candidate ns/commit | Change | 95% interval |
|---|---:|---:|---:|---:|
| 0 bytes | 28,692.68 | 28,417.48 | −0.6054% | [−1.9900%, +0.7124%] |
| 32 bytes | 28,015.75 | 28,094.60 | +0.5496% | [−2.5104%, +3.8254%] |
| 4 KiB | 31,175.44 | 30,088.53 | −3.5690% | [−4.8884%, −1.9167%] |
| 1 MiB | 893,482.87 | 496,097.97 | −44.5091% | [−44.9358%, −43.8670%] |

Small-blob results are inconclusive. These results describe commits reusing a
blob, not new-blob commits, all persistence operations, or whole-application
Penelope overhead. They cannot be added to earlier percentage improvements.

## Validation and reproduction

The existing ten-case stored-column matrix checks healthy, wrong, empty, short,
long, oversized, TEXT, INTEGER, REAL and missing values through reads and replay.
Retained malformed rows are also rejected by duplicate commits without changing
records, heads or blobs. Each case now additionally rejects an unverified caller
buffer, including when it equals the malformed stored bytes. The missing-row case
continues to exercise read/replay rejection; it is not evidence for warm-cache
append rejection.

The zero-byte and 1 MiB boundary fixtures now also exercise successful duplicate
checkpoint commits, rejection after stored corruption with unchanged durable
rows, authoritative repair, successful reuse, reconstruction and reopen. Twenty
isolated test processes, ten per compiler, run all ten matrix and two boundary
fixtures: 200 matrix cases and 40 boundary fixtures.

Run the benchmark with:

```sh
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo bench -p penelope-sqlite --bench task_blob_commits
```

The evidence directory contains raw measurements, analysis, build/source/binary
hashes, environment information, reproduction scripts and qualification logs.
Run its `verify.py` to check archive integrity and independently recompute all four
statistics. Historical source pins are retained; a later production change does
not make these measurements evidence for that later source.

Qualification passed: stable full workspace/all targets/all features (excluding
the separately configured fuzz package); Rust 1.85 SQLite all targets/all
features (24 unit, 51 process and 34 task tests plus custom bench executables);
strict workspace Clippy; formatting; layer boundaries; and graph update. The
final added caller-buffer assertions also passed in the 20 isolated runs.
The code preceding this change passed CI run 41172225982; this report does not
claim a completed CI run for the new commits.
