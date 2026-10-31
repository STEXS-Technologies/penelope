# SQLite claim UPDATE statement reuse — 2026-10-31

Fix: `1fb0849ccb691e941530e7119a9382aa52ea83b3`. Baseline: `4b63eac65464123cce5bff3665e59272608a4244`.

## Change and guarantees

Outbox and timer claims already reuse their selection and token-counter statements. Their lease UPDATE was still prepared and finalized once per returned item. Nonempty claims now acquire that UPDATE with `prepare_cached`, hold it across the batch, then return it to the connection's existing bounded statement cache before committing. Empty claims skip UPDATE preparation entirely. SQL text and bound values are unchanged.

This removes repeated SQL compilation and performs only one UPDATE cache lookup per nonempty claim. Each item still reads the authoritative token counter, checks overflow, conditionally increments it, serializes/validates its lease and requires exactly one lease row to change. Selection integrity checks, attempt limits, writer transactions, WAL/FULL settings, trusted-clock sampling and worker backpressure remain in place. No authority, tokens or rows are newly cached in application memory. There is no schema, cache-capacity or public API change. Prepared statements consume space within the existing bounded connection cache and may displace other statements in mixed workloads; this campaign does not quantify that tradeoff.

## Correctness

The existing warmed-token rollback/retry/reopen integration test now also suppresses the second lease UPDATE. For outbox and timer modes it first claims three records, warming the statements; an external connection then installs each of two triggers in turn: ignore the second token increment or ignore the second lease UPDATE. The first item has already updated successfully when either fault occurs. Each rejected batch must return `Invariant`, preserve all previous lease bytes/attempts/expiries and leave next_token at 4. Removing the triggers permits retry with exactly tokens 4–6 and attempt/expiry 2/121; reopening permits tokens 7–9 with 3/132. This also exercises external schema invalidation of warmed statements.

The expanded test passes against the baseline too: it is a compatibility guard for an optimization, not a new correctness failure attributed to old code. Twenty isolated processes, ten on stable and ten on Rust 1.85.0, pass the expanded fixture: 80 injected fault/rollback scenarios and 40 retry/reopen workflows. Existing qualification retains corruption, token exhaustion, suppressed writes, cancellation and recovery coverage.

Stable workspace all-target/all-feature tests excluding the separate fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting, layer boundaries and graphify update pass. After the baseline-source compatibility control restores candidate code, complete SQLite unit/process/task suites pass again on both compilers: 31 unit, 51 process and 42 task tests. Measured production/test/benchmark hashes match the final restored sources. These rollback checks do not independently prove crash durability.

## Measurements

Three separate campaigns use the same four Rust 1.99 release executables and unchanged public `lease_claim`/`lease_batch` fixtures on Ryzen 9 7950X, Linux 7.2.9, CPU 0 affinity inherited by caller and worker. Each campaign has 24 alternating AB/BA pairs: 96 isolated processes and 288 observations; combined, 288 processes and 864 observations. No concurrent Penelope builds/tests, graph updates or bootstrap analyses ran during any measurement campaign. All pairs are retained. Unrelated system activity, device/cache state and power-loss behavior are not controlled.

The first campaign uses the default `/tmp`, which is tmpfs on this machine. It exposes CPU/dispatch/SQLite costs without durable-device synchronization and must not be presented as durable disk throughput. The second and third explicitly set TMPDIR to the project qualification directory on NVMe-backed btrfs. All retain the adapter's WAL/FULL configuration. The filesystem capture is archived. Filesystem synchronization noise can dominate these small operations.

Each empty observation measures 2,000 public claims. Each nonempty profile measures 400 expired-batch reclaims across eight fresh stores after 10 per-store warmup claims. Setup and post-call assertions are untimed. The unchanged fixtures validate batch lengths, outbox attempts, unique lease tokens and exact persisted counters. These are repeated expired claims, not complete delivery/acknowledgement workflows, cold-start tests or retained-history/mixed-workload studies. Batch size is leases per API call.

Changes are means of within-pair candidate/baseline latency percentages, not ratios of aggregate means. Negative means faster. Intervals are deterministic 20,000-resample percentile bootstraps. They describe individual comparisons without multiplicity correction. Rust 1.85 is correctness-tested, not performance-tested. Allocation counts are not instrumented. No global Penelope slowdown or universal overhead bound follows from these operation-specific results.

### tmpfs

| Claim | Batch | Baseline µs/call | Candidate µs/call | Change | 95% interval |
|---|---:|---:|---:|---:|---:|
| empty outbox | 0 | 15.624 | 15.537 | -0.54834% | [-1.05629, -0.06408]% |
| empty timer | 0 | 16.109 | 16.432 | +1.98715% | [-0.90541, +6.92640]% |
| outbox | 1 | 31.661 | 29.067 | -8.17409% | [-9.02776, -7.45128]% |
| outbox | 16 | 114.976 | 90.830 | -20.99591% | [-21.42450, -20.56339]% |
| timer | 1 | 32.035 | 29.387 | -8.25954% | [-8.88070, -7.60869]% |
| timer | 16 | 106.631 | 83.564 | -21.62808% | [-21.99289, -21.25605]% |

### NVMe-backed btrfs — first campaign

| Claim | Batch | Baseline µs/call | Candidate µs/call | Change | 95% interval |
|---|---:|---:|---:|---:|---:|
| empty outbox | 0 | 16.174 | 15.799 | -0.44734% | [-6.19785, +5.95056]% |
| empty timer | 0 | 16.329 | 16.170 | -0.45351% | [-2.13064, +0.57009]% |
| outbox | 1 | 852.726 | 885.363 | +7.87726% | [-1.60532, +17.45380]% |
| outbox | 16 | 994.056 | 993.174 | +2.49993% | [-7.06233, +13.04871]% |
| timer | 1 | 952.862 | 839.409 | -5.50140% | [-16.61465, +5.95329]% |
| timer | 16 | 995.001 | 916.932 | -6.31583% | [-13.53113, +1.34371]% |

### NVMe-backed btrfs — repeat

| Claim | Batch | Baseline µs/call | Candidate µs/call | Change | 95% interval |
|---|---:|---:|---:|---:|---:|
| empty outbox | 0 | 15.503 | 16.154 | +4.16170% | [-0.66611, +11.88982]% |
| empty timer | 0 | 15.914 | 16.342 | +2.68313% | [-0.46884, +8.19647]% |
| outbox | 1 | 829.892 | 845.142 | +4.09016% | [-4.27105, +12.63285]% |
| outbox | 16 | 1045.848 | 944.303 | -5.20832% | [-14.66367, +4.78110]% |
| timer | 1 | 920.353 | 847.261 | -1.17260% | [-12.59834, +10.24541]% |
| timer | 16 | 1025.167 | 983.802 | +0.12295% | [-10.93222, +12.60226]% |

The tmpfs nonempty profiles show clear reductions: about 8.2% for single-item claims and 21–22% for batches of 16. The small empty-outbox reduction is not presented as an algorithmic gain; empty UPDATE preparation was already absent in the baseline.

Every interval in both btrfs campaigns includes zero. In particular, single-item outbox changes are +7.87726% and +4.09016%, with wide intervals; these observations cannot rule out a meaningful device-backed regression or establish a device-backed speedup. Batch and timer point estimates also vary between campaigns. No pairs or adverse estimates are discarded, and the campaigns are not pooled to replace their separate results.

The change is retained because it eliminates repeated SQL compilation with clear CPU-sensitive latency gains and preserves the existing authority/rollback contracts. A net benefit for device-backed or mixed application workloads remains unproven. The measured reductions must not be described as global Penelope overhead reductions.

The baseline's GitHub Actions run `41180237471` completed successfully, including bounded fuzzing and optional StateChronicle conformance. Its archived result applies to the baseline, not CI of the new optimization. No fresh local fuzz or optional StateChronicle gate is claimed.

[Evidence and reproduction](verification-data/2026-10-31-claim-update-cache/README.md) contains scripts, source snapshots, executable hashes, all raw measurements, qualification logs and a standalone verifier that recomputes all 18 comparisons. The release plans in Downloads were not copied into the repository.
