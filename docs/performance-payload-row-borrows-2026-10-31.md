# SQLite payload row borrows — 2026-10-31

Code: `e425a04f4c468b84564c476706c31e94fdab5d05`. Baseline:
`ea5a230e847a6eca9c088e8811c458d92fc37d87`. Evidence is in
`verification-data/2026-10-31-payload-row-borrows/`.

An exact payload put retry previously copied the stored BLOB into a temporary
Rust vector just to compare it with the already-owned submitted bytes. A corrupt
read copied its stored bytes before checking the digest. These paths now use
the existing bounded row accessor: retry compares inside the row callback, and
read verifies the row's digest before allocating its successful owned result.

SQL BLOB type and size checks are unchanged. Caller digest validation and copying
submitted bytes before queueing remain intact, as do quota admission, required
single-row write checks, transactions, and scope binding. Valid reads still
return an independent owned vector. SQLite may allocate/copy its internal BLOB
representation; this is not a zero-copy SQLite implementation. No schema or
public API change is introduced.

## Validation and accounting

The same tests pass against the baseline and both candidates. An eight-case
stored-row matrix covers valid, short, empty, changed, oversized, TEXT, INTEGER,
and REAL values. It checks read/retry results, unchanged payload and usage rows,
repair followed by successful replay/retry, ownership of a previously returned
vector, and retry at exactly the scope-byte quota. Invalid caller digests and
new payload admission beyond that quota remain errors.

Separate zero-byte and 1 MiB cases check valid retry/read, external corruption,
repair/reopen, returned-vector independence, and unchanged usage charges.
Twenty isolated final-candidate processes, ten/compiler, pass another 200 fixture
cases (eight matrix cases plus two boundary fixtures per process).

Final stable workspace/all-target/all-feature tests and Rust 1.85.0 SQLite
all-target/all-feature tests pass, including 24 SQLite unit tests, 51 process
tests, and 26 task tests on each compiler. Strict workspace Clippy, formatting,
layer boundaries, and graph update pass. Production and benchmark source hashes
match the release measurement inputs exactly.

## Measurement design

The shared fixture opens fresh WAL/FULL databases and seeds one payload through
the public port. Sizes are 0, 32, 4,096, and 1,048,576 bytes. Each tests an exact
put retry, valid read, missing-digest read, corrupt read, and retry against corrupt
stored content. Corruption is an external SQL fixture mutation outside timing;
for a zero-byte original it stores one wrong byte. After each condition the
fixture checks stored bytes and usage. Finally it repairs the row and verifies
read/retry again. Caller result verification is outside measurement.

The ordinary-allocator latency executable includes the public async call,
request/future/queue/wake overhead, SQLite work and digest/copy operations. Below
1 MiB it uses 100 warmups and 1,000 calls; at 1 MiB it uses 32 warmups and 128
calls. Construction, seed/corruption/repair, and caller verification are excluded.
A separate global-allocator executable uses sixteen warmups and sixteen samples
per condition, counting Rust allocations, reallocations and cumulative requested
bytes across the caller and worker. Those instrumented runs supply no latency
claims, do not count SQLite's C heap, and do not measure peak/resident memory.

The first candidate returned a nested result from the read callback. It was
measured against baseline in 24 alternating AB/BA pairs: 96 processes, including
separate allocation probes. The final candidate uses the existing SQLite
corruption classification for digest mismatch, avoiding the second result type.
It preserves the same public Invariant error. A new matched campaign compares
baseline, original candidate, and final candidate in 24 blocks, with all six
orders repeated four times: 144 processes. The original binaries are reused,
and all three variants use identical fixture sources. This is not a comparison
inferred by subtracting independent campaigns.

Both complete campaigns are retained: 240 processes, 80 latency comparisons and
38,400 allocation samples. Processes and worker threads inherit CPU 0 affinity
on AMD Ryzen 9 7950X, Linux 7.2.9, rustc 1.99.0. No own builds/tests/graph jobs
run during either timing campaign. These are single-caller/single-CPU SQLite
payload fixtures, not overall Penelope overhead, concurrent throughput, or
performance for arbitrary data/cache states.

## Rust allocation traffic

Every sample agrees on the expected delta: one allocation of the stored byte
length disappears on affected nonempty retry/rejection paths. Valid reads and
missing reads have unchanged counts. Empty valid payloads allocate no payload
buffer in either variant; their corrupted fixture contains one byte, whose
allocation is removed. Reallocations remain unchanged.

The final 1 MiB fixture gives:

| Operation | Rust allocations before → after | Reallocations | Requested bytes before → after |
|---|---:|---:|---:|
| duplicate | 8 → 7 | 4 | 2097872 → 1049296 |
| read | 7 → 7 | 4 | 1049168 → 1049168 |
| missing | 6 → 6 | 4 | 592 → 592 |
| corrupt_read | 7 → 6 | 4 | 1049168 → 592 |
| corrupt_put | 8 → 7 | 4 | 2097872 → 1049296 |

Each affected 1 MiB call removes 1,048,576 bytes of Rust allocation traffic.
The submitted owned buffer is still needed for queued puts; healthy reads still
allocate their returned vector. Fixed request/response allocations also remain.

## Final latency and tradeoffs

Negative means lower latency. Changes are means of 24 matched process-level
ratios minus one, not ratios of grand means. Brackets are percentile 95%
bootstrap intervals from 10,000 resamples, seed 42. Individual calls are not
treated as independent statistical replicates. Intervals are per condition
without multiple-comparison correction.

| Payload bytes | Operation | Final change % | 95% interval % |
|---:|---|---:|---:|
| 0 | duplicate | -0.335 | [-1.145, +0.444] |
| 0 | read | +0.735 | [+0.202, +1.364] |
| 0 | missing | +0.408 | [+0.038, +0.809] |
| 0 | corrupt_read | +0.590 | [-0.139, +1.252] |
| 0 | corrupt_put | +0.515 | [+0.029, +1.029] |
| 32 | duplicate | -0.055 | [-0.667, +0.499] |
| 32 | read | +0.262 | [-0.264, +0.750] |
| 32 | missing | +0.257 | [-0.400, +0.940] |
| 32 | corrupt_read | +0.015 | [-0.512, +0.541] |
| 32 | corrupt_put | -0.438 | [-1.056, +0.180] |
| 4096 | duplicate | -0.138 | [-0.631, +0.369] |
| 4096 | read | +0.412 | [-0.133, +1.009] |
| 4096 | missing | +0.636 | [+0.160, +1.220] |
| 4096 | corrupt_read | -0.501 | [-1.143, +0.155] |
| 4096 | corrupt_put | -0.555 | [-1.179, +0.151] |
| 1048576 | duplicate | -3.169 | [-3.546, -2.787] |
| 1048576 | read | +0.434 | [+0.078, +0.766] |
| 1048576 | missing | +0.277 | [-0.304, +0.863] |
| 1048576 | corrupt_read | -3.344 | [-3.592, -3.109] |
| 1048576 | corrupt_put | -3.374 | [-3.736, -3.023] |

At 1 MiB the final candidate improves exact retry by 3.169%, corrupt read by
3.344%, and retry against corrupt content by 3.374%, with intervals below zero.
The first campaign's original candidate shows corresponding gains of 3.219%,
3.346%, and 3.099%. The complete final campaign also includes direct comparisons
with that candidate; the simpler error representation is retained for clarity,
not a claimed additional speed improvement over it.

There are small measured costs. Valid 1 MiB reads change from 443,998.1 to
445,911.0 ns/call: +0.434% [0.078%, 0.766%]. Valid empty reads are +0.735%
[0.202%, 1.364%]. Empty corrupt retries and some missing-read controls also show
small positive intervals in the final table. Eliminating a payload copy does
not explain the missing-read controls, which never fetch a matching BLOB; these
results are retained rather than attributed to a copy-saving mechanism.
Most other small-payload results are inconclusive. This change is retained for
reduced temporary allocation traffic and the measured large retry/rejection
benefit, with the successful-read tradeoff documented.

## Evidence and reproduction

The data directory contains full source pins, all candidate snapshots, source/
binary hashes, production/fixture patch, raw process/build/check/regression logs,
exact campaign scripts, and every summary. The verifier checks source identities,
recomputes all 80 statistics from raw outputs, checks all 38,400 allocation
samples and expected byte/count deltas, and verifies the regression records.

From the repository root:

```sh
python3 docs/verification-data/2026-10-31-payload-row-borrows/verify.py
```

For a fresh rebuild on the pinned source, run archived `reproduce-builds.py` from
the repository root. It builds the original archived candidate, restores final
production source in `finally`, then builds the final candidate while retaining
the original binaries. Run `compare-payload-borrow.py` and then
`compare-payload-borrow-final.py` to repeat the two campaigns. Scripts write
ignored `target/payload-borrow-*` files. Do not run concurrent source mutations
or builds during rebuilding, or other own jobs during timing. Recorded hashes
identify this run, not a promise of identical binaries on another environment.
