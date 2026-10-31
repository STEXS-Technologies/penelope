# SQLite public claim latency and write tradeoffs — 2026-10-31

The schema-v5 indexes reduce empty public claim latency by about 79% with
1,024 retained records in this local fixture. They also increase commits of
128 timer actions by **21.8465% [8.7913%, 36.4813%]**. This is an adapter
read/write tradeoff, not an overall Penelope overhead measurement.

A follow-up experiment reused prepared statements for required single-row
writes. It did not establish a write-latency benefit against the indexed
baseline, so **the experiment was rejected**. Production retains the original
`execute_one` implementation and all existing one-row guards. The benchmark commit is `fa817f7`. The committed
change adds a reusable public-port benchmark and evidence; it introduces no new
runtime optimization, schema, API, or durability change.

## Sources and decision

Three complete campaigns are reported separately:

- **A: indexes alone.** Pre-index `4f1a0f39cc84974d81aaf67c48f9ecee3db41fda`
  (schema 4) versus indexed `5966fe2b32b3b5ac53c3ed049294e20e5ab25618`
  (schema 5). Only `schema.rs` was swapped; the latter source includes schema-v5
  migration/index creation and its test-only migration cuts.
- **B: indexes plus experimental caching.** The same pre-index source versus
  the indexed source with `execute_one` changed from `Connection::execute` to
  `prepare_cached(...).execute(...)`. The exact rejected source is archived.
- **C: caching isolated.** Indexed source versus the same experimental caching,
  both schema 5. This is the direct comparison used to reject caching.

B still measured timer-action commits **17.9169% slower [9.2459%, 26.6832%]**
than pre-index. C's apparent timer commit reduction was **−4.5585%
[−12.5213%, 4.0750%]**, and its outbox commit and scheduling intervals also
crossed zero. C additionally measured timer batch-16 claims **+7.5499%
[0.5614%, 14.0997%]**. These results do not justify retaining the experiment.
C's outbox-open increase has a particularly wide interval; opening precedes
required writes in this fixture, so this is not evidence of a causal cache
startup cost. All metrics, including noisy and adverse results, follow below.
Independent campaigns cannot be subtracted or multiplied to infer an isolated
effect. No paired trials were discarded.

The existing indexes remain because they bound claim selection work as retained
history grows; the earlier deterministic VM-step evidence is in
[the index report](performance-sqlite-claim-indexes-2026-10-31.md).
Applications dominated by small write-heavy batches may pay more than they gain.
This follow-up quantifies that limitation rather than claiming a universal
speedup. A direct application workload is needed to measure net overhead.

## Method

Rust 1.99.0 release binaries, bundled SQLite, AMD Ryzen 9 7950X, local temporary
WAL/FULL databases. Each campaign contains 20 alternating AB/BA pairs. Each
variant runs the existing `lease_batch` benchmark followed by the new
`claim_history` benchmark: 80 successful processes per campaign, 240 total.
Processes and their worker threads inherit one permitted CPU affinity. No other
agent builds, tests, or graph jobs ran during timing. Filesystem synchronization,
OS cache state, and scheduling are not controlled; intervals show substantial
variation in write paths. Rust 1.85.0 is correctness-tested, not performance-tested.

`claim_history` uses typed public ports to populate each fresh store with
1,024 actions in eight commits of 128 actions and one outcome each. The first
commit accepts a canonical input; subsequent outcomes refer to that input.
For the outbox fixture, eight claims return exactly 128 actions each in order,
then all 1,024 actions are individually acknowledged. The timer fixture schedules
all 1,024 timer actions at time 200 and polls at time 100; its outgoing actions
remain pending. It does not execute or acknowledge timers. These are storage
fixtures, not complete external-effect or timer-delivery workflows.

After 100 warmup empty claims, it times 1,000 empty calls. Each must return no
records. Read-only SQLite inspection afterward verifies all 1,024 retained rows,
the expected state/lease/attempt columns, schema version, and exact token counter
(1,025 outbox; 1 timer). No raw SQL writes seed these fixtures.

Open, commit, scheduling, initial claims and acknowledgements are timed separately.
The timed public calls include worker dispatch, validation, SQLite transactions,
index maintenance, synchronization, and returned results. Commit fixture
construction and receipt validation are excluded. Idle timing includes the loop,
future handling, returned-vector destruction and empty-result check. Values are
normalized by API calls: eight commits, eight initial outbox claims, 1,024
acknowledgements or schedules, and 1,000 idle claims. A commit call contains
128 actions; values are not per action. Store opening is measured once per fixture.
The commit profile contains only eight batches; it is not a steady-state history
growth or cold/warm-cache isolation study.

`lease_batch` retains its existing semantics: four outbox/timer × batch-1/16
profiles, eight fresh stores each, 10 warmup and 50 timed expired reclaims per
store, 400 timed calls per profile. It validates batch lengths, attempts, unique
tokens and exact persisted counters. Setup and post-call checks are excluded.

Changes below average 20 within-pair candidate/baseline ratios, not ratios of
aggregate means. Intervals bootstrap those ratios 10,000 times with seed 42,
using sorted positions 250 and 9,749. All 13 metrics per campaign are included.
Intervals are individual/descriptive without multiplicity correction. The tables
do not establish performance on other hardware, storage, competing writers,
batch sizes, retained histories, or complete workflows.

## Results

Negative latency change is faster. Means are microseconds per API call.
`history/false` means outbox; `history/true` means future timers.
`seed_claim` is the initial outbox claim before acknowledgement. `batch` is
expired reclaiming; `history/*/idle` is empty polling with retained rows.

### A — indexes alone

| Metric | Baseline µs/call | Candidate µs/call | Mean paired change | 95% interval |
| --- | ---: | ---: | ---: | ---: |
| `batch/false/1` | 799.711 | 824.121 | +6.8828% | [-4.8393%, +19.4203%] |
| `batch/false/16` | 924.267 | 993.677 | +8.6653% | [-2.4173%, +21.8160%] |
| `batch/true/1` | 867.809 | 801.890 | -2.9134% | [-13.2526%, +8.6805%] |
| `batch/true/16` | 939.206 | 874.686 | -4.6955% | [-11.8769%, +3.3864%] |
| `history/false/ack` | 881.170 | 933.587 | +7.6836% | [-5.7129%, +25.1059%] |
| `history/false/commit` | 2800.443 | 3082.975 | +10.4769% | [-4.3025%, +32.4163%] |
| `history/false/idle` | 87.514 | 17.739 | -79.3375% | [-80.9587%, -77.5084%] |
| `history/false/open` | 6058.996 | 5444.022 | -5.1566% | [-15.9351%, +5.6202%] |
| `history/false/seed_claim` | 2958.464 | 2299.891 | -20.9502% | [-30.0786%, -8.8222%] |
| `history/true/commit` | 2802.085 | 3335.714 | +21.8465% | [+8.7913%, +36.4813%] |
| `history/true/idle` | 91.460 | 18.848 | -78.9034% | [-80.8866%, -76.8170%] |
| `history/true/open` | 7823.645 | 6922.715 | -4.9652% | [-18.2806%, +9.5798%] |
| `history/true/schedule` | 934.872 | 916.091 | +0.2471% | [-7.8543%, +8.8836%] |

### B — indexes plus rejected cache

| Metric | Baseline µs/call | Candidate µs/call | Mean paired change | 95% interval |
| --- | ---: | ---: | ---: | ---: |
| `batch/false/1` | 802.507 | 879.745 | +11.5690% | [-1.4718%, +27.7250%] |
| `batch/false/16` | 977.012 | 905.382 | -5.1383% | [-13.4545%, +3.2222%] |
| `batch/true/1` | 851.275 | 775.245 | -5.5304% | [-14.4560%, +2.8358%] |
| `batch/true/16` | 917.930 | 876.159 | -3.0324% | [-11.2431%, +5.1526%] |
| `history/false/ack` | 828.508 | 850.835 | +5.0002% | [-4.0374%, +14.4997%] |
| `history/false/commit` | 2609.043 | 2415.448 | -6.0326% | [-13.1005%, +0.9820%] |
| `history/false/idle` | 78.524 | 16.744 | -78.5913% | [-80.0010%, -76.7628%] |
| `history/false/open` | 5503.177 | 5420.966 | +5.6214% | [-10.0953%, +23.7937%] |
| `history/false/seed_claim` | 2712.156 | 2156.371 | -17.0792% | [-27.0146%, -6.3988%] |
| `history/true/commit` | 2365.804 | 2771.102 | +17.9169% | [+9.2459%, +26.6832%] |
| `history/true/idle` | 80.808 | 17.632 | -78.0454% | [-79.6436%, -76.0955%] |
| `history/true/open` | 6479.393 | 6818.831 | +9.2433% | [-5.2225%, +25.3608%] |
| `history/true/schedule` | 799.500 | 851.049 | +7.0733% | [-0.3001%, +14.2190%] |

### C — rejected cache versus indexed baseline

| Metric | Baseline µs/call | Candidate µs/call | Mean paired change | 95% interval |
| --- | ---: | ---: | ---: | ---: |
| `batch/false/1` | 907.078 | 916.805 | +4.3883% | [-7.2726%, +17.8029%] |
| `batch/false/16` | 1071.555 | 1059.765 | +1.8359% | [-7.2200%, +11.2494%] |
| `batch/true/1` | 899.904 | 878.379 | +0.9472% | [-8.7745%, +11.6350%] |
| `batch/true/16` | 984.432 | 1047.547 | +7.5499% | [+0.5614%, +14.0997%] |
| `history/false/ack` | 919.671 | 983.410 | +8.4600% | [-1.2633%, +18.5903%] |
| `history/false/commit` | 2621.699 | 2514.077 | -3.3536% | [-9.4118%, +2.5311%] |
| `history/false/idle` | 15.764 | 15.820 | +0.3315% | [-1.7653%, +3.3971%] |
| `history/false/open` | 5625.532 | 6646.138 | +30.5523% | [+1.4418%, +62.4198%] |
| `history/false/seed_claim` | 2258.295 | 2168.499 | -1.8745% | [-9.4791%, +6.3261%] |
| `history/true/commit` | 2948.678 | 2790.797 | -4.5585% | [-12.5213%, +4.0750%] |
| `history/true/idle` | 16.479 | 16.539 | +0.3482% | [-0.9962%, +1.9756%] |
| `history/true/open` | 6802.062 | 6506.217 | +0.5690% | [-13.5874%, +15.9723%] |
| `history/true/schedule` | 978.787 | 922.388 | -4.6694% | [-10.6414%, +1.3091%] |

## Validation and retained evidence

The benchmark and unchanged indexed production code passed full workspace
all-target/all-feature suites excluding `penelope-fuzz`, with compiler wrappers
cleared and `RUST_TEST_THREADS=1`, on Rust 1.99.0 and 1.85.0. Each compiler passed
21 SQLite unit, 39 process integration, and 26 task integration tests, including
suppressed required writes, rollback/retry/reopen, migration interruption, quota,
lease eligibility and token contracts. Benchmark debug runs in these suites are
functional checks, not additional controlled timing trials. Workspace Clippy
with `-D warnings`, formatting, layer boundaries, and `graphify update .` passed.
No new fault-matrix repetition is claimed for the rejected experiment.

The caching candidate passed an earlier SQLite suite and Clippy after fixing
five benchmark-local shadowed clock-variable names. The original Clippy failure,
fixed run, and both benchmark snapshots are retained. The only benchmark change
between A and B/C renamed those local clock variables. Initial unused cache-build
logs are retained separately; campaign metadata identifies the binaries actually
run. The candidate `candidate_source` field records its parent HEAD during the
uncommitted experiment, **not a clean candidate commit**; compiled source hashes
and archived snapshots identify it exactly. The rejected source never entered
production history.

[Evidence directory](verification-data/2026-10-31-sqlite-claim-latency) includes
all raw process outputs, build scripts/logs, source snapshots and hashes, binary
hashes (executables are not in Git), metadata, summaries, validation logs, and
checksums. Scripts retain the original local paths. To reproduce a campaign,
apply its archived candidate schema to its indexed parent, add the corresponding
benchmark and manifest entry, then run the archived build/compare scripts;
build scripts temporarily swap and restore `schema.rs`. Use an isolated checkout.
Compiler/storage/hardware differences can change results.

Verify all checksums, complete pairs, source/binary pins, fixture counters, both
compiler suites, and recompute all 39 metrics and intervals:

```sh
python3 docs/verification-data/2026-10-31-sqlite-claim-latency/verify.py
```
