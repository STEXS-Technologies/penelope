# Borrowed SQLite replay columns — 2026-10-31

Code: `47f58fbdb3f32dfb0f31fbf26433c780727c743f`. Baseline:
`ec4399f0cc27698a63b7c9ac23e408b0b6557785`. Raw evidence and verifier are in
`verification-data/2026-10-31-borrowed-replay-columns/`.

Replay now checks bounded SQLite BLOB slices and decodes directly from the row,
instead of copying each encoded record and checksum into two temporary vectors.
The returned process outcomes remain owned. Rust lifetimes keep the borrowed
bytes inside the row's lifetime; no unsafe production code is introduced.
The existing copy helper delegates to the same bounded accessor for callers
that need ownership. SQL type and size checks precede decoding as before.

The cached query, transaction snapshot, head/tail check, checksum, typed record
validation, sequence/scope checks, and continuation rules remain intact. There
is no schema or public API change.

## Separate allocation and latency measurements

Both release variants use the same shared public-port replay fixture. Population
uses typed commits in fresh WAL/FULL databases; histories contain 1, 128, or 4,096
outcomes. Each tests first, last, exact-end, and past-end cursors with limit 1,
and a first page with limit 128. At history 1, first/last are identical requests
and the limit-128 page returns only one record. These are labeled fixture
conditions, not independent application workloads.

The ordinary-allocator latency executable performs 100 warmups and 1,000 timed
public replay calls per condition. Timing includes request/future construction,
queue/wake overhead, worker query and validation, and owned page construction.
Caller verification of exact scope, cursor, rows, and continuation happens after
timing. Opening/population are excluded; a final read-only count checks history
has not changed.

A separate instrumented executable counts Rust allocation traffic around the
same public replay call, with a 128-outcome history, 100 warmups per condition,
and 16 samples. It does not supply latency results. It counts allocations,
reallocations, and cumulative requested bytes across Rust threads. It does not
count SQLite's C heap, measure peak/resident memory, or claim a zero-allocation
public API. Page verification and expected encoded byte counts are outside the
counting region.

There are 24 matched pairs, alternating AB/BA. For each variant the ordinary
latency executable runs first, then the separate allocation executable: 96
processes total, 720,000 timed replay calls and 3,840 allocation samples. All
raw outputs are retained. Processes and workers inherit CPU 0 affinity on an AMD
Ryzen 9 7950X, Linux 7.2.9, rustc 1.99.0. No own builds, tests, or graph jobs run
during timing. This single-caller/single-CPU fixture does not establish overall
application overhead, concurrency/cold behavior, or other record shapes.

## Allocation traffic

All 384 samples per variant/condition agree on these values. Requested bytes
include reallocations; reallocations are unchanged by the optimization.

| Page at history 128 | Rust allocations before → after | Reallocations | Requested bytes before → after | Temporary bytes removed |
|---|---:|---:|---:|---:|
| first | 11 → 9 | 4 | 2492 → 2056 | 436 |
| last | 11 → 9 | 4 | 2500 → 2056 | 444 |
| end | 8 → 8 | 4 | 1000 → 1000 | 0 |
| past | 8 → 8 | 4 | 1000 → 1000 | 0 |
| page | 265 → 9 | 9 | 124244 → 67528 | 56716 |

The 128-record page removes exactly 256 allocations and 56,716 requested bytes:
265 → 9 allocations and 124,244 → 67,528 requested bytes per call. Each returned
record removes two allocations, covering its encoded bytes and checksum. Empty
pages perform no row copies and their counts remain unchanged.

## Public replay latency

Negative change means lower latency. Values are mean paired changes, with
percentile 95% bootstrap intervals from 10,000 draws of the 24 process-level
ratios (seed 42). Individual calls are not treated as independent statistical
replicates. Intervals are per condition, without multiple-comparison correction.

| History | Cursor/page | Change % | 95% interval % |
|---:|---|---:|---:|
| 1 | first | +1.609 | [-0.994, +6.016] |
| 1 | last | -0.333 | [-1.104, +0.374] |
| 1 | end | -0.504 | [-1.332, +0.257] |
| 1 | past | +0.728 | [-0.825, +3.085] |
| 1 | page | -0.433 | [-1.208, +0.279] |
| 128 | first | +1.536 | [-0.939, +5.493] |
| 128 | last | +1.410 | [-1.021, +5.555] |
| 128 | end | -0.720 | [-1.465, -0.076] |
| 128 | past | -0.488 | [-1.196, +0.203] |
| 128 | page | -1.789 | [-2.218, -1.388] |
| 4096 | first | -0.459 | [-1.426, +0.398] |
| 4096 | last | -0.863 | [-1.765, -0.027] |
| 4096 | end | -1.389 | [-2.171, -0.734] |
| 4096 | past | -1.011 | [-1.808, -0.318] |
| 4096 | page | -1.928 | [-2.380, -1.500] |

Full 128-record pages improve by 1.789% at history 128 and 1.928% at history 4,096,
with intervals entirely below zero. At history 4,096, mean time changes from
180,415.0 to 176,918.7 ns/call. This is the supported latency benefit for this
slice; most single-record results are inconclusive.

Some empty-page controls also show lower times even though they execute neither
changed row-access call. Those changes are preserved but are not attributed to
removing row copies. No claim of a general single-record improvement or a new
overall Penelope slowdown percentage follows from this campaign.

## Validation and compatibility

The same 20-case validation matrix passes against both baseline and candidate.
It covers original and exactly-at-limit valid records, over-limit/empty/malformed
JSON, JSON null/trailing data, invalid UTF-8, wrong scope/sequence, TEXT/INTEGER/
REAL record values, and wrong-type/empty/short/wrong/oversized checksums. Digests
for malformed encoded data match those bytes, so checksum failure cannot mask
parser/semantic rejection. Cases keep the same warm statement, assert replay
preserves stored rows, restore the record and checksum, and then replay again.
Previously returned owned pages also remain unchanged after database mutation.

Twenty isolated candidate processes (ten per compiler) pass another 400 cases.
Existing warm-cache, head/tail, and other SQLite regression tests also pass.
The stable workspace/all-target/all-feature test suite and MSRV 1.85.0 SQLite
all-target/all-feature suite pass, including 23 SQLite unit tests, 45 process
tests, and 26 task tests on each compiler. Strict workspace Clippy, formatting,
layer boundaries, and graph update pass. Final production and fixture hashes
match the release measurement inputs exactly.

## Evidence and reproduction

The data directory contains full source pins and snapshots, fixture/production
patch, binary and source hashes, raw build/process/check logs, exact build and
campaign scripts, summaries, and a verifier. The verifier checks all 96 processes,
recomputes all 15 latency statistics and intervals, checks all allocation deltas,
and checks baseline/candidate validation and regression records.

From the repository root:

```sh
python3 docs/verification-data/2026-10-31-borrowed-replay-columns/verify.py
```

To rebuild and repeat on the same source, run archived `build-replay-borrow.py`
then `compare-replay-borrow.py` from the repository root. They write ignored
`target/replay-borrow-*` files. The build script temporarily selects historical
production files and restores the candidate in `finally`; do not run other
source mutations or builds concurrently. Keep own builds/tests/graph jobs out
of timing. Reproducing requires a local release build; recorded binary hashes
identify this campaign rather than promising identical binaries elsewhere.
