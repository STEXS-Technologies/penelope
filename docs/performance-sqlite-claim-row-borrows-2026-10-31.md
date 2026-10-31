# SQLite claim row borrowing — 2026-10-31

Fix: `d84111a202d40c15bfcc8935d2581126e4b8f76b`. Baseline: `0a8bb67393becd1d22108db948ab4e83fd35f6a7`.

## Change

Outbox and due-timer selection used `copy_column` for both the serialized record and its checksum, then immediately decoded the record. Those temporary vectors do not contribute to the owned action/timer returned by the decoder. Both loops now use the existing `column_bytes` helper and decode directly from the live SQLite row. Only leases.rs changes in production.

`copy_column` itself already delegates to `column_bytes` before making a vector. Thus BLOB type checks, byte limits, error mapping and subsequent raw-byte checksum/JSON/identity validation remain the same. The owned decoded values enter the existing pending batch; no SQLite borrow survives the row or reaches the asynchronous caller. There is no new unsafe production code, schema, cache, public API or concurrency policy. Selection order, attempt limits, exact lease metadata, token allocation and transactional writes remain unchanged. ACK/renew validation is not altered.

## Allocation evidence

Separate instrumented `lease_batch` executables count Rust allocations, reallocations and requested allocation bytes across the public call and its worker operation. They use the repository's existing atomic System allocator instrumentation, injected into an archived copy of the otherwise unchanged fixture. Counting starts just before the public call and stops when it returns an owned lease vector, before result checking/destruction and probe output. Timings from instrumented executables are ignored. This measures allocation traffic, not live/peak memory, deallocations, SQLite's C allocator or total process RSS.

Four alternating AB/BA allocation pairs contain eight processes and 512 samples. Each variant contributes 64 samples per profile: two post-warmup calls in each of eight fixtures across four processes. Every sample within a profile has identical counts. Both variants retain the fixture's result, attempt, unique-token and persisted-counter assertions. The borrowed production path removes exactly two Rust allocations per returned item, with no reallocation change.

| Claim | Batch | Allocations baseline → candidate | Reallocations baseline → candidate | Requested bytes baseline → candidate | Saved bytes |
|---|---:|---:|---:|---:|---:|
| outbox | 1 | 16 → 14 | 10 → 10 | 3748 → 3360 | 388 |
| outbox | 16 | 106 → 74 | 102 → 102 | 38584 → 32376 | 6208 |
| timer | 1 | 15 → 13 | 6 → 6 | 3553 → 3152 | 401 |
| timer | 16 | 90 → 58 | 38 → 38 | 35056 → 28640 | 6416 |

These byte reductions apply to the archived fixture's short identifiers/records. Other records have different serialized lengths. The instrumented source, its build logs and strict benchmark Clippy result are archived; allocator instrumentation is not added to production or used in ordinary latency measurements.

## Latency checks

Uninstrumented release executables compare the same baseline and candidate using unchanged `lease_claim` and `lease_batch` fixtures. Both campaigns use `/tmp` on tmpfs, Rust 1.99, Ryzen 9 7950X, Linux 7.2.9 and CPU 0 affinity inherited by caller and worker. The adapter still configures WAL/FULL, but tmpfs does not represent durable device synchronization. No device-backed speed or universal application-overhead claim follows.

Each campaign has 24 alternating AB/BA pairs, 96 isolated processes and 288 observations. Together they contain 192 processes and 576 observations. No concurrent Penelope builds/tests, graph updates, allocator probes or bootstrap analyses ran during timing. All observations are retained. Each empty observation measures 2,000 calls; each nonempty profile measures 400 expired-batch claims across eight fixtures after 10 warmup calls per fixture. Setup and post-call checks are untimed. Batch size is leases per API call. Existing fixture assertions validate returned results, delivery attempts, token uniqueness and exact persisted counters.

Changes are means of within-pair candidate/baseline percentages. Negative means faster; aggregate means are also shown. Intervals are deterministic 20,000-resample percentile bootstraps, individual/descriptive without multiplicity correction. The first campaign had positive, unresolved point estimates in several nonempty profiles; the confirmation campaign was run to check that uncertainty. Both are reported separately without discarding trials. Storage state and unrelated system activity are not controlled. Rust 1.85 is correctness-tested, not performance-tested.

### First campaign

| Claim | Batch | Baseline µs/call | Candidate µs/call | Change | 95% interval |
|---|---:|---:|---:|---:|---:|
| empty outbox | 0 | 15.992 | 16.038 | +0.48909% | [-2.35238, +4.35217]% |
| empty timer | 0 | 16.286 | 16.474 | +1.16583% | [-0.79568, +4.67004]% |
| outbox | 1 | 29.550 | 31.238 | +5.68604% | [-1.50828, +14.00500]% |
| outbox | 16 | 93.039 | 98.831 | +6.30909% | [-1.59933, +16.28061]% |
| timer | 1 | 29.941 | 31.546 | +5.24275% | [-1.62857, +15.29188]% |
| timer | 16 | 86.968 | 86.831 | +0.55816% | [-4.41948, +6.50793]% |

### Confirmation campaign

| Claim | Batch | Baseline µs/call | Candidate µs/call | Change | 95% interval |
|---|---:|---:|---:|---:|---:|
| empty outbox | 0 | 15.830 | 15.838 | +0.06733% | [-0.72974, +0.97522]% |
| empty timer | 0 | 16.320 | 16.244 | -0.45398% | [-0.90894, -0.00608]% |
| outbox | 1 | 29.629 | 29.270 | -1.17554% | [-2.13391, -0.17476]% |
| outbox | 16 | 92.975 | 91.136 | -1.94456% | [-2.83672, -1.11988]% |
| timer | 1 | 30.194 | 29.592 | -1.92005% | [-3.09690, -0.83121]% |
| timer | 16 | 85.734 | 84.245 | -1.66775% | [-2.92117, -0.60178]% |

The first campaign resolves neither a speedup nor a slowdown in any profile. The confirmation campaign estimates nonempty reductions from 1.17554% to 1.94456%, with negative intervals in all four nonempty profiles. Its small empty-timer reduction is not presented as an algorithmic gain: the changed decoding path does not run for empty results. The initial campaign remains part of the evidence, with its adverse point estimates and wider intervals.

The change is retained for its consistent, directly measured allocation savings. Confirmation timing supports a modest local latency gain, but this campaign pair does not establish a universal speedup, device-backed benefit or global Penelope slowdown percentage. The two campaigns are not pooled to replace their separate results.

## Qualification

Stable workspace all-target/all-feature tests excluding the separate fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting, layer boundaries and graphify update pass. Both compilers pass 31 SQLite unit, 51 process and 42 task tests. Twenty additional isolated processes, ten per compiler, each pass the 13 existing lease-contract tests: 260 test executions. This retains corrupted-pending-record, damaged-index-binding, lease-completion evidence, stale/renewed envelope, overflow, suppressed-write and warmed multi-item rollback/retry/reopen coverage. No new correctness failure is attributed to the baseline.

The measured production sources and ordinary benchmark fixtures match the final sources after the temporary probe source is restored. Baseline GitHub Actions run `41180940020` completed successfully, including bounded fuzzing and optional StateChronicle conformance; its archived result is baseline CI, not new-code CI. No fresh local fuzz or optional StateChronicle gate is claimed. Existing recovery tests supply separate durability coverage; allocation/timing fixtures do not independently prove crash recovery.

[Evidence and reproduction](verification-data/2026-10-31-claim-row-borrows/README.md) includes exact ordinary and instrumented sources, executable hashes, all raw samples, qualification logs, scripts and a standalone verifier. The release plans in Downloads were not copied into the repository.
