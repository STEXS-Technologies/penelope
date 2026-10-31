# SQLite replay statement cache — 2026-10-31

Code: `a20bb8c7a09f8bcdd62ddd12346f1a59533b7e0e`.
Previous head/tail guard: `465b6b267afe68796e21e133f971d5ad8796cbc9`.
Before that guard: `837265bfc82658a6d1fe02e4776372fe4673d805`.

Outcome replay now reuses its prepared page-selection statement. Every request
still binds scope, cursor, and limit and reads authoritative rows within the
existing transaction. The new head/tail guard, checksum verification, decoding,
record validation, sequence checks, and continuation rules remain intact.
Only preparation changes in production; no schema or API changes.

## Public-port fixture

`process_replay` creates fresh WAL/FULL adapter databases and populates 1, 128,
and 4,096 typed outcomes through public atomic commits, in batches up to 128.
Each history tests first and last records, exact-end and past-end empty cursors
with limit 1, and a first page with limit 128. At history 1 the first and last
labels exercise identical requests; the limit-128 page also contains one record.
There are 15 labeled conditions, not 15 independent application workloads.

Each condition has 100 warmups and 1,000 measured calls. The measured region
includes the public async request, queue/wake overhead, worker query, record
checks/decoding, and page construction. An independent caller check of exact
scope, cursor, contents, and continuation happens after each timed call. A final
read-only SQL count checks that replay did not change history. Population, open,
and caller verification are outside the timing region.

Release binaries use rustc 1.99.0, an ordinary allocator, and the same benchmark
source. Processes and their worker threads inherit CPU 0 affinity on an AMD
Ryzen 9 7950X under Linux 7.2.9. This single-caller/single-CPU adapter fixture does
not establish application-wide overhead, concurrent throughput, cold latency,
or performance on other hardware/record shapes. No own builds, tests, or graph
jobs ran during either timing campaign.

## Matched results

The main campaign uses 24 matched blocks across three variants: before the
head/tail guard, with the guard, and with the guard plus replay cache. All six
variant orders repeat four times, for 72 processes. The comparison with the
previous guard commit isolates this change. An independent confirmation was
declared once after seeing the main result, using the same guard/cache binaries
for 24 alternating AB/BA pairs (48 processes). All trials from both campaigns
are retained. Across both there are 1,800,000 timed replay calls, plus warmups.

The table reports mean paired latency change versus the guard commit; negative
means faster. Brackets are percentile 95% bootstrap intervals from 10,000 draws
of the 24 process-level ratios (seed 42). Individual calls are not treated as
independent statistical replicates. Intervals are per condition, without a
multiple-comparison correction.

| History | Cursor/page | Main change % [95% CI] | Confirmation change % [95% CI] |
|---:|---|---:|---:|
| 1 | first | -2.524 [-4.115, -0.697] | -3.182 [-4.080, -2.232] |
| 1 | last | -2.963 [-3.953, -1.778] | -2.968 [-3.691, -2.278] |
| 1 | end | -5.505 [-6.212, -4.797] | -5.537 [-6.229, -4.842] |
| 1 | past | -5.737 [-6.565, -5.009] | -5.885 [-7.600, -4.671] |
| 1 | page | -2.797 [-3.717, -1.885] | -3.049 [-3.981, -2.144] |
| 128 | first | -2.371 [-8.103, +4.019] | -3.411 [-4.335, -2.565] |
| 128 | last | -3.722 [-7.540, -0.931] | -2.887 [-4.106, -1.433] |
| 128 | end | -6.100 [-9.638, -3.981] | -5.189 [-5.825, -4.558] |
| 128 | past | -6.360 [-9.752, -4.306] | -5.344 [-6.180, -4.493] |
| 128 | page | +0.308 [-3.318, +3.080] | +0.223 [-0.269, +0.636] |
| 4096 | first | -1.794 [-3.472, +0.802] | -3.585 [-4.423, -2.749] |
| 4096 | last | -1.528 [-2.429, -0.629] | -2.664 [-3.340, -2.002] |
| 4096 | end | -4.456 [-5.395, -3.509] | -4.510 [-5.662, -3.422] |
| 4096 | past | -3.841 [-5.006, -2.643] | -4.348 [-5.066, -3.657] |
| 4096 | page | +0.825 [+0.167, +1.522] | +0.204 [-0.333, +0.723] |

Small and empty pages show repeatable gains. The confirmation estimates range
from 2.664% to 5.885% lower latency across those 13 labeled conditions. For example,
the 4,096-history last-record page changes from 16,004.6 to 15,576.3 ns/call in the
confirmation, with a paired change of −2.664% [−3.340%, −2.002%].

Full 128-record pages show no established gain. The main 4,096-history page has
a +0.825% change [+0.167%, +1.522%], which is preserved as evidence of a possible
small regression. The independent confirmation is +0.204% [−0.333%, +0.723%];
its 128-history full page is +0.223% [−0.269%, +0.636%]. These results neither
establish a repeatable large-page slowdown nor prove absence of a small one.
The cache is retained for the repeatable small/empty-page improvement.

The main campaign also measures the prior head/tail guard against its own
baseline. That change includes caching the larger metadata query and is not
purely an added-check cost: ten conditions improve with intervals below zero,
and five are inconclusive. Comparing the final cache build directly with the
pre-guard build gives twelve improvements with intervals below zero and three
inconclusive conditions. All 45 main statistics and 15 confirmation statistics,
including means and absolute times, are archived. No single overall Penelope
slowdown percentage follows from these measurements.

## Correctness and compatibility

A warm-cache regression alternates scopes, cursors, and limits; detects a changed
checksum after warming; successfully reads another scope and an unaffected page
after that failure; recovers after checksum repair; survives external index
creation/removal; and observes appended outcomes and updated continuations on
the same open adapter. It passes in twenty isolated processes, ten each on stable
and Rust 1.85.0. The existing damaged-head/tail regressions also pass with the
cached page query.

The stable workspace/all-target/all-feature test suite and the MSRV SQLite
all-target/all-feature suite pass. SQLite reports 23 unit tests, 44 process tests,
and 26 task tests on both. Strict workspace Clippy, formatting, layer boundaries,
and graph update pass. Benchmark and production source hashes match the measured
builds exactly; the evidence verifier recomputes all 60 statistics from raw logs.

## Evidence and reproduction

`verification-data/2026-10-31-replay-statement-cache/` contains full source pins,
source snapshots, binary hashes, build logs, fixture patch, raw process outputs,
all statistics, regression/check logs, and the exact build/campaign scripts.
The compiler/binary hashes support this run; reproduction requires a fresh build
on the local environment. Timing values from debug all-target test executions
are qualification output, not campaign evidence.

Run from the repository root:

```sh
python3 docs/verification-data/2026-10-31-replay-statement-cache/verify.py
```

To rebuild and repeat on the same source, run the archived `build-replay-cache.py`,
then `compare-replay-cache.py`, then `confirm-replay-cache.py` from the repository
root. They write ignored `target/replay-cache-*` files and restore production
source after temporarily building the historical variants. Do not run source
mutations or other builds concurrently with that build script, or other own
jobs during timing. Repetition is not a requirement to obtain a preferred result;
the original two campaigns remain authoritative evidence for these numbers.
