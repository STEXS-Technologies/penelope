# Pending-inbox index — 2026-10-31

Code: `02416c007df7262b46a154555b0104dbe8ee1022`. Baseline:
`bd0cdf5bfcced391d59ce11d58831dbc6a764f88`. Evidence is in
`verification-data/2026-10-31-pending-inbox-index/`.

Pending-input discovery previously used the primary key to find a process's
inbox rows, then scanned consumed inputs while filtering `commit_digest IS NULL`.
An empty poll therefore scanned retained consumed history. The adapter now adds:

```sql
CREATE INDEX process_inbox_pending
ON process_inbox(process_key,input_id)
WHERE commit_digest IS NULL;
```

The discovery SQL, row decoding/validation, limit, and ID ordering are unchanged.
The index contains only inputs available for discovery; an atomic consuming
commit removes its entry while retaining the authoritative input row.

## Migration and compatibility

Schema version six builds the index and updates `user_version` inside the existing
migration transaction. Index creation failure or page exhaustion preserves the
old version and logical rows. Versions above six remain unsupported. Existing
version-one/three/four migration fixtures now remove the new index when constructing
an older schema and assert version six after successful upgrade.

The populated version-five migration test preserves pending inputs, exact retry
receipts, and live outbox/timer leases, then consumes one pending input and checks
reopen/retry/discovery again. Separate fixtures exercise an index-name collision,
a database page cap requiring additional capacity, and rejection of a healthy
version-seven database without row changes. Collision and page-cap fixtures
recover after repair/raising the bound.

Crash tests kill a version-one upgrade at eleven boundaries and a populated
version-five upgrade at seven boundaries, including after index creation, after
version change, and after commit. They check old/new versions, index presence,
logical rows, and successful reopen. The version-five crash seed contains one
real typed consumed commit and 128 valid pending inputs; pending admission in
that engine fixture is synthetic with the normal usage charge. Public-port
migration/admission behavior is covered separately.

Twenty isolated port-test processes (ten/compiler) pass 80 fixtures. Ten isolated
migration-test processes (five/compiler) pass 180 actual SIGKILL cuts. Stable
workspace/all-target/all-feature tests and Rust 1.85.0 SQLite all-target/all-feature
tests pass, including 24 SQLite unit tests, 49 process tests, and 26 task tests
on both compilers. Strict workspace Clippy, formatting, layer boundaries, and
graph update pass.

The additive migration preserves rows but changes the format version: older
builds reject version-six databases on open. Index creation needs disk capacity
and scans the existing inbox during migration. Populated migration latency is
not measured here; fresh-open measurements below are a different condition.

## Public-port benchmark

`pending_history` opens fresh WAL/FULL databases with 0, 128, and 1,024 consumed
inputs. Every input is accepted through `Inbox.accept` and consumed by a typed
atomic commit. The fixture measures empty discovery, then accepts sixteen new
inputs in reverse order, verifies repeated discovery in ascending ID order,
and consumes them through typed commits. Receipts are validated outside the
timed regions. Discovery verifies exact returned input values after timing.
Final read-only checks assert every input is consumed and retained.

Each discovery condition has 100 warmups and 1,000 measured public calls.
Acceptance and consuming-commit timings are reported separately for the retained
prefix and the final sixteen inputs; constructing caller inputs/commits and
validating receipts are excluded. Public async call/worker/queue/wake, SQLite
work, and decoding remain inside the timed regions. Fresh opening is separate.

The main campaign has 24 matched pairs alternating AB/BA. After observing a
write-cost signal, one independent confirmation was declared with the same
binaries and another 24 alternating pairs. Both complete campaigns are retained:
96 processes, 576,000 timed discoveries, 115,200 accepts, and 115,200 consuming
commits. Each process runs the three histories in the same order. Processes and
workers inherit CPU 0 affinity on AMD Ryzen 9 7950X, Linux 7.2.9, rustc 1.99.0.
No own build/test/graph jobs run during either timing campaign.

This is a single-caller/single-CPU local-adapter fixture. It does not establish
application-wide overhead, multi-process throughput, cold discovery performance,
or other input/history distributions. It specifically places consumed IDs before
pending IDs to exercise retained-history scanning.

## Results and tradeoffs

Changes are means of 24 paired indexed/baseline ratios minus one; they are not
ratios of grand means. Brackets are percentile 95% bootstrap intervals from
10,000 process-level resamples, seed 42. Individual calls are not independent
statistical replicates. Intervals are per condition without multiple-comparison
correction. Negative means lower latency.

| Consumed prefix | Operation | Main change % [95% CI] | Confirmation change % [95% CI] |
|---:|---|---:|---:|
| 0 | empty | +2.377 [+1.934, +2.812] | +3.725 [+1.400, +7.663] |
| 0 | pending | +0.839 [+0.294, +1.346] | +2.423 [-0.591, +7.343] |
| 0 | pending_accept | -1.977 [-17.055, +15.084] | +2.091 [-14.531, +23.435] |
| 0 | pending_commit | +5.381 [-6.796, +17.948] | +6.070 [-8.000, +21.041] |
| 0 | open | +23.773 [+8.121, +41.081] | +26.515 [+3.660, +50.380] |
| 128 | empty | -32.851 [-33.169, -32.530] | -30.712 [-33.048, -27.680] |
| 128 | pending | -18.232 [-18.918, -17.709] | -16.225 [-19.246, -12.016] |
| 128 | seed_accept | -2.870 [-13.835, +8.573] | +8.305 [-6.393, +26.914] |
| 128 | seed_commit | -0.062 [-10.593, +11.280] | +8.370 [-6.247, +25.980] |
| 128 | pending_accept | -2.163 [-12.845, +8.838] | +12.723 [-3.520, +31.005] |
| 128 | pending_commit | +8.291 [-4.998, +22.446] | +15.677 [-11.283, +52.745] |
| 128 | open | -8.683 [-22.490, +6.776] | +12.572 [-16.302, +56.940] |
| 1024 | empty | -80.124 [-80.262, -79.975] | -79.582 [-80.081, -78.817] |
| 1024 | pending | -65.103 [-65.280, -64.906] | -64.436 [-65.756, -62.718] |
| 1024 | seed_accept | +3.399 [-5.752, +12.404] | +3.861 [-4.215, +12.414] |
| 1024 | seed_commit | +2.061 [-5.470, +9.315] | +5.260 [-3.829, +14.785] |
| 1024 | pending_accept | -2.828 [-16.868, +13.014] | +6.163 [-10.502, +24.815] |
| 1024 | pending_commit | +23.594 [+7.919, +39.842] | -9.000 [-18.948, +1.296] |
| 1024 | open | +13.642 [-3.003, +32.146] | +11.602 [-12.189, +38.830] |

The retained-history read gains repeat: with 1,024 consumed inputs, confirmation
empty discovery is 79.582% faster and discovery of sixteen pending inputs is
64.436% faster. The corresponding 128-history gains are 30.712% and 16.225%.
These are discovery gains, not whole-application speedups.

There are costs. With no retained history, empty discovery is 3.725% slower in
the confirmation [1.400%, 7.663%], following +2.377% in the main campaign.
First fresh-open latency increases by 26.515% [3.660%, 50.380%], following
+23.773% in the main campaign. The other fresh-open conditions are inconclusive.
The zero-history pending-batch signal also does not clearly repeat.

Write cost remains uncertain. The main final-sixteen consuming-commit condition
at history 1,024 is +23.594% [7.919%, 39.842%], but confirmation is −9.000%
[−18.948%, 1.296%]. Both results are retained; they do not establish either a
repeatable slowdown in that condition or absence of index maintenance overhead.
All confirmation acceptance/commit intervals span zero, and several are wide.
The index is retained for the large repeatable improvement as consumed history
grows, with the measured startup/empty-history costs and write uncertainty.

## SQL work and storage

The benchmark executes the exact production SELECT separately, checks the IDs,
and records bundled SQLite 3.53.2 VM instructions and query plans. Every trial
agrees on the following counts; neither variant sorts:

| Consumed prefix | Empty before → indexed | Sixteen pending before → indexed |
|---:|---:|---:|
| 0 | 11 → 11 | 169 → 137 |
| 128 | 650 → 11 | 809 → 137 |
| 1024 | 5130 → 11 | 5289 → 137 |

Baseline plans use `sqlite_autoindex_process_inbox_1`; indexed plans use
`process_inbox_pending`. These instruction counts show the eliminated scan in
these fixtures, not constant physical I/O cost for arbitrary index sizes.
After all fixture inputs are consumed, database pages are 42 → 43, 87 → 88,
and 387 → 388 respectively. This one-page difference describes these typed
fixtures with at most sixteen pending inputs at a time; it is not a capacity
estimate for a large pending backlog.

## Evidence and reproduction

The evidence directory contains full source pins and snapshots, production/
fixture patch, source/binary hashes, raw process/build/check/crash logs, exact
build and campaign scripts, and all 38 latency statistics. The verifier checks
both campaigns, recomputes the paired statistics and intervals, checks the SQL
matches production and the engine plans/counts, and verifies migration evidence.

From the repository root:

```sh
python3 docs/verification-data/2026-10-31-pending-inbox-index/verify.py
```

To rebuild and repeat on the same source, run archived `build-inbox-index.py`,
then `compare-inbox-index.py`, then `confirm-inbox-index.py` from the repository
root. They write ignored `target/inbox-index-*` files. The build script temporarily
selects historical `schema.rs` and restores the candidate in `finally`; do not
run concurrent source mutations/builds. Keep other own jobs out of timing.
Recorded binary hashes identify this campaign; reproduction needs a local build.
