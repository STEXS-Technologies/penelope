# SQLite claim serialization buffer reuse — 2026-10-31

Fix: `e2ea47715de232ca7b5285d6aa3af7a43302c1e0`. Baseline: `7608a47201e07b6509aca530731745bd919b5f2d`.

## Change and invariants

Each outbox or timer lease previously serialized into a fresh vector, including every item in one claimed batch. A nonempty claim now creates one 128-byte-capacity vector and reuses it for all items. It clears the vector before each serialization, lets it grow as needed, applies the existing per-record byte-length check, binds its slice and executes the lease UPDATE before reusing the buffer. The buffer is local to one batch and is dropped before commit; it does not persist across requests, scopes or connections. Empty claims do not allocate it.

The private `encode_into` helper also backs `encode`: the owned wrapper starts with capacity 128, matching the pinned serde_json `to_vec` implementation, and both use the same serde_json writer. Serialized JSON and error mapping remain the same. This is a post-serialization size check, just as before; it does not introduce a hard preallocation/memory ceiling or a streaming quota. Codec errors return `Invariant`; oversized output returns `QuotaExceeded`. Either error in a claim aborts its existing transaction.

SQL, parameters, statement reuse, exact-row guards, token issuance, attempts, integrity checks, expiry rules, WAL/FULL configuration and worker admission are unchanged. The returned leases remain independently owned. There is no schema/public API change or new unsafe production code. The helper refactor applies to other existing process encoders too; their owned-vector behavior and byte limits are preserved.

## Correctness evidence

Two new unit tests compare reused and owned output with serde_json bytes for long/short strings, nested JSON, Unicode/escaping and scalars at just-below, exact and above size boundaries. They also inject a codec error after partial output, exceed a quota and verify that a subsequent short serialization contains no old bytes. Zero limits and recovery after error are covered.

A new public-port integration fixture runs both outbox and timer workflows. Its ordered batch contains a long first action and a short second action; owners also shrink between initial claim and reclaim. Starting the counter at 99 crosses a token digit-width boundary. Every stored lease BLOB must exactly match independent serialization of its returned lease. Reclaimed tokens/attempts must be correct, prior envelopes must be fenced, and reopening must permit ACK of both current leases with counter exactly 103. This tests both shorter byte sequences and SQLite binding ownership across buffer reuse.

The integration fixture also passes against baseline process.rs/leases.rs: this is an optimization preserving existing behavior. A deliberate candidate mutation omitting `bytes.clear()` fails the stored-byte equality check on the second outbox lease. That control stops at the first outbox failure; it does not independently exercise a mutant timer case. Controls always restore production source in `finally`, and restored unit/integration tests pass afterward.

Ten isolated serialization/integration process pairs per compiler (stable and Rust 1.85.0) give 40 child processes and 60 test executions. They include 20 integration runs covering 40 outbox/timer workflows, each with initial claim, reclaim, stale rejection and reopened ACK. Existing transaction rollback, suppressed-write, corruption and recovery tests also pass. These specific fixtures establish byte compatibility, fencing and visibility, not an independent power-loss proof.

## Allocation traffic

Separate instrumented copies of the unchanged public `lease_batch` fixture count Rust allocations, reallocations and requested bytes across each public call and worker operation. Counting stops before result checking/destruction and output. Four AB/BA pairs contain eight allocation processes and 512 samples; each profile/variant contributes 64 samples. Every sample within a profile has identical counts. Instrumented elapsed times are ignored; allocator instrumentation is not installed in production or ordinary latency binaries.

| Claim | Batch | Allocations baseline → candidate | Reallocations baseline → candidate | Requested bytes baseline → candidate | Saved bytes |
|---|---:|---:|---:|---:|---:|
| outbox | 1 | 14 → 14 | 10 → 10 | 3360 → 3360 | 0 |
| outbox | 16 | 74 → 59 | 102 → 72 | 32376 → 18936 | 13440 |
| timer | 1 | 13 → 13 | 6 → 6 | 3152 → 3152 | 0 |
| timer | 16 | 58 → 43 | 38 → 8 | 28640 → 15200 | 13440 |

For batches of 16, both profiles save 15 allocations, 30 reallocations and 13,440 requested bytes per API call. Single-item counts are unchanged. These are allocation-traffic measurements for the archived records; they do not measure live/peak memory, deallocations, SQLite C allocation or RSS. Record sizes and growth thresholds affect other workloads. Fixture assertions still check results, attempts, token uniqueness and exact persisted counters.

## Latency

Both uninstrumented campaigns use the same four Rust 1.99 release executables on Ryzen 9 7950X, Linux 7.2.9, CPU 0 affinity inherited by caller and worker. Each has 24 alternating AB/BA pairs, 96 isolated processes and 288 observations; together, 192 processes and 576 observations. Each empty observation measures 2,000 calls. Each nonempty profile measures 400 expired claims across eight fixtures after 10 per-fixture warmup claims. Setup and result assertions are untimed. Batch size is leases per API call.

Both use `/tmp` on tmpfs. The adapter retains WAL/FULL, but tmpfs does not represent durable-device synchronization. No concurrent Penelope builds/tests, graph updates, allocator probes or bootstrap analyses ran during timing. All observations are retained. Unrelated system activity and storage/cache state are not controlled. Rust 1.85 is correctness-tested, not performance-tested. These are adapter operations, not complete delivery workflows or global Penelope overhead measurements.

Changes are means of within-pair candidate/baseline percentages, not ratios of aggregate means. Negative means faster. Intervals use deterministic 20,000-resample percentile bootstraps and describe individual comparisons without multiplicity correction. The confirmation campaign follows full qualification and uses the exact same executables; both campaigns are reported separately.

### First campaign

| Claim | Batch | Baseline µs/call | Candidate µs/call | Change | 95% interval |
|---|---:|---:|---:|---:|---:|
| empty outbox | 0 | 16.590 | 17.150 | +4.02045% | [-1.34961, +10.97911]% |
| empty timer | 0 | 17.216 | 17.454 | +2.60557% | [-3.25730, +9.90504]% |
| outbox | 1 | 31.528 | 31.241 | +1.64028% | [-6.32182, +10.10387]% |
| outbox | 16 | 100.642 | 92.023 | -6.20977% | [-12.00971, -1.15289]% |
| timer | 1 | 32.027 | 30.120 | -3.95631% | [-8.95843, -0.32478]% |
| timer | 16 | 92.417 | 82.968 | -7.74542% | [-13.05169, -3.64352]% |

### Confirmation campaign

| Claim | Batch | Baseline µs/call | Candidate µs/call | Change | 95% interval |
|---|---:|---:|---:|---:|---:|
| empty outbox | 0 | 17.324 | 17.060 | -1.15180% | [-3.84584, +1.42503]% |
| empty timer | 0 | 18.168 | 17.567 | -2.40917% | [-5.20222, +0.06358]% |
| outbox | 1 | 33.282 | 31.598 | -3.47127% | [-7.74354, +0.22278]% |
| outbox | 16 | 105.811 | 95.910 | -7.74921% | [-11.95247, -4.30347]% |
| timer | 1 | 33.023 | 32.369 | -0.74829% | [-4.60342, +2.63661]% |
| timer | 16 | 90.458 | 90.077 | -0.79941% | [-3.88617, +3.45782]% |

Batch-16 outbox claims improve in both campaigns: -6.20977% and -7.74921%, with negative intervals. Timer batch-16 latency improves in the first campaign (-7.74542%) but is unresolved in the confirmation (-0.79941% with an interval spanning zero). That does not establish a replicated timer latency gain. Single-item allocation counts are unchanged; their variable timing estimates are not presented as a targeted allocation improvement. Empty profiles show no resolved effect.

The optimization is retained for its consistent reduction in batch allocation/reallocation traffic and the repeated local outbox-batch latency gain. Neither campaign resolves a slowdown, but the intervals do not establish a universal nonregression bound. No device-backed benefit or global Penelope slowdown percentage is established, and the two campaigns are not pooled to replace their separate results.

## Qualification and reproduction

Stable workspace all-target/all-feature tests excluding the separate fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting, layer boundaries and graphify update pass. Both compilers pass 33 SQLite unit, 52 process and 42 task tests. Dedicated Clippy also passes with the instrumented allocation fixture temporarily installed, then restored. Measured production, test and ordinary fixture hashes match the final sources after control/probe restoration.

Baseline GitHub Actions run `41181596846` completed successfully, including bounded fuzzing and optional StateChronicle conformance; its archived result applies to the baseline, not the new optimization. No fresh local fuzz or optional StateChronicle gate is claimed.

[Evidence and reproduction](verification-data/2026-10-31-claim-encode-reuse/README.md) includes sources, executable hashes, all raw allocation/latency observations, baseline/mutation controls, qualification logs, scripts and a standalone verifier. The release plans in Downloads were not copied into the repository.
