# Borrowed pending-input discovery with lazy result allocation — 2026-10-31

Baseline: `0506449983da1c89ad5bd19392e76f0e8e2a3487`. Candidate: `170531404a21775c457d3ea4ba5f2ab339432f6e`.

Pending-input discovery already uses one SELECT for each bounded, scoped, ID-ordered page. It previously prepared that SELECT afresh and copied each retained input/checksum into temporary vectors before decoding. It now reuses the prepared statement and borrows bounded BLOBs from the current SQLite row. The decoded inputs remain owned return values; no row data, decoded result or successful validation is cached across calls.

The result vector is also allocated lazily, after the first valid input is decoded and checked. Empty polls return an empty vector without reserving an unused page buffer. Nonempty pages still reserve the requested bounded capacity once. SQL predicates/order/limit, checksum validation, per-record/checksum bounds, SQLite storage-class checks and tenant/process/input identity checks are unchanged. The row/statement finish within the existing worker callback. No schema or public API changes are made.

## Ordinary release latency

The final candidate is compared with the baseline using the same typed public fixture and ordinary timing executable, without a counting allocator. Each of three profiles has a fresh database with 0, 1 or 16 pending inputs, inserted through public Inbox admission in reverse ID order. Every profile requests a limit of 16. After 64 warmups, 5,000 public discovery calls must each return the exact expected sorted inputs. Final durable input/consumption counts must remain unchanged.

Two campaigns each run 12 four-process ABBA/BAAB blocks: 96 isolated processes, 288 timing profile rows and 1,440,000 measured calls in total. Every process runs empty, single and 16-input profiles in that order; all final-comparison samples are retained. Caller and worker inherit CPU 0 affinity. Temporary databases use project qualification storage on NVMe btrfs, not /tmp tmpfs. CPU frequency and background system load are uncontrolled. No own builds, tests, graph updates, allocation probes or bootstrap analysis ran during timing.

Per-block changes compare geometric means of two candidate and two baseline times per call. Campaign changes average the 12 log ratios; 95% intervals use 20,000 deterministic block-bootstrap resamples and the 2.5th/97.5th percentiles. Negative changes mean lower latency.

| Campaign | Pending inputs | Baseline ns/call | Candidate ns/call | Change | 95% interval |
|---|---:|---:|---:|---:|---:|
| 1 | 0 | 11484.3 | 10624.3 | -7.48805% | [-8.62773%, -6.66442%] |
| 1 | 1 | 12603.2 | 11965.3 | -5.06137% | [-6.14046%, -4.27540%] |
| 1 | 16 | 23126.1 | 22244.5 | -3.81220% | [-5.02308%, -2.96495%] |
| 2 | 0 | 11279.8 | 10451.9 | -7.33929% | [-7.80185%, -6.91757%] |
| 2 | 1 | 12525.9 | 11825.9 | -5.58791% | [-6.95834%, -4.66665%] |
| 2 | 16 | 22752.9 | 21982.8 | -3.38454% | [-3.58576%, -3.17902%] |

All six confidence intervals are below zero: both campaigns support lower latency for these warm empty, single-input and 16-input discovery fixtures.

These are warm public discovery calls for small pending queues with a fixed page limit. They do not establish first-call latency, retained-history scaling, different core placements, arbitrary storage performance or a whole-application Penelope slowdown percentage. The experiment combines statement reuse, borrowed BLOBs and lazy allocation; it does not separately attribute their timing contributions.

## Allocation evidence

Four final allocation-probe processes per revision, three profiles and 32 samples per profile after 64 warmups produce 768 samples. Every sample has the same tuple for its revision/profile; none are excluded.

| Pending inputs, requested limit 16 | Rust allocations before → after | Reallocations before → after | Requested bytes before → after |
|---|---:|---:|---:|
| 0 | 7 → 7 | 0 → 0 | 2,128 → 480 |
| 1 | 9 → 8 | 0 → 0 | 2,338 → 2,272 |
| 16 | 39 → 8 | 0 → 0 | 5,488 → 2,272 |

The 16-input page saves 31 allocations and 3,216 requested bytes per call; a single-input page saves one allocation and 66 bytes. Empty polling keeps the same allocation count while saving 1,648 requested bytes. Statement reuse itself adds Rust allocation traffic; lazy allocation removes the unused empty-page buffer and offsets that cost. These are net public-call results.

Counters observe Rust System allocation/reallocation requests across caller and worker threads. SQLite's C allocator and peak/live memory are outside the counter; requested-byte totals are allocation traffic, not retained memory. These instrumented runs are separate from ordinary timing.

## Reliability qualification

The extended integrity test warms a two-input page, then damages its later row through a second connection, leaving the first row valid. Ten fault types cover checksum damage, indexed-ID substitution, oversized payload, foreign tenant, wrong checksum storage class, oversized checksum, wrong payload storage class, malformed JSON with a valid checksum, foreign process and short checksum. The oversized fixture is valid, correctly checksummed JSON padded with whitespace to one byte beyond the configured default record limit, so its rejection specifically depends on the bound. Warmed and reopened workers must both return Invariant, with exact snapshots of all ten process/lease-counter tables unchanged and no partial successful page.

A new lifecycle test warms an empty reader, admits inputs B and A through another worker, and checks empty → [B] → [A,B] → [B] → empty as public atomic commits consume them. A reopened reader must also see the remaining input and final empty state. Existing discovery tests retain batch-limit, scope, ordering and consumed-input coverage.

Twenty isolated processes, ten stable and ten Rust 1.85.0, pass all three pending-inbox tests: 60 test executions, including 200 late-row fault fixtures/400 typed rejections and 20 external-worker refresh lifecycles. The complete 60-test process suite passes against both baseline and candidate with the expanded coverage.

Two mutation controls separately bypass checksum comparison or disable the input BLOB size bound. They incorrectly return a successful page in the damaged-checksum or valid-oversized-JSON fixture, respectively, and fail the test. Each control stops at its first intentional failure and does not execute every faulty combination. Production source is restored in `finally`; the restored full process suite passes. The baseline passes the same tests: this removes temporary work while preserving existing validation and discovery behavior.

Stable workspace all-target/all-feature tests excluding the fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting, layer boundaries and graphify update pass. Both compilers pass 33 SQLite unit, 60 process and 42 task tests. Baseline GitHub Actions run `41185796124` completed successfully, including bounded fuzzing and optional StateChronicle conformance; its status applies to the baseline before this optimization. No fresh local fuzz or optional StateChronicle conformance run is claimed. Debug benches in all-target qualification are functional executions, excluded from the controlled timing analysis.

[Evidence and reproduction](verification-data/2026-10-31-pending-validation/README.md) includes final comparison sources/hashes, raw timing and allocation samples, metadata/bootstrap analysis, intentional-control logs, repeated regressions, qualification logs and a standalone verifier. This is not an exhaustive interleaving, corruption reconstruction or power-loss proof. Plans from Downloads are not included.
