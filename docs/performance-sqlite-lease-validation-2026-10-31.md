# Borrowed SQLite lease validation — 2026-10-31

Baseline: `f383d51fb03af753b2600a02e47a5efa7c208788`. Candidate: `fbae67835fae4f4ab356fff43fc4e1841f9bcccb`.

Outbox acknowledgement/renewal and timer acknowledgement previously copied all fetched BLOBs before validating durable evidence. They now borrow those BLOBs from the current SQLite row using the existing bounded `column_bytes` helper and cache the validation SELECT statements. The row, statement and borrowed data stay inside the existing immediate transaction and return before any completion UPDATE. No borrowed row escapes, and no decoded record or validation result is cached across calls. Queries, predicates, field-read order, limits, envelope equality, checksum/domain checks and mutation guards remain the same. Schema and public APIs are unchanged.

The outbox path removes four BLOB-to-Vec copies; the timer path removes three. Statement-cache entry/return costs also affect the net public-call allocation count, so the measured saving is smaller than the number of removed copies.

## Allocation evidence

A standalone release probe calls typed public ports through the normal database worker and WAL/FULL transactions. Each of three cases uses a fresh temporary database populated by public commit/claim/schedule calls. It warms 16 calls, then records 32 allocation samples. Four separate processes per revision produce 768 samples in total. All samples for each revision/case have the same allocation tuple; none are discarded. The same probe source and allocator instrumentation are used for both revisions.

| Public call | Allocations before → after | Reallocations before → after | Requested bytes before → after |
|---|---:|---:|---:|
| outbox acknowledgement | 13 → 10 | 14 → 14 | 3,036 → 2,176 |
| timer acknowledgement | 11 → 9 | 10 → 10 | 2,633 → 1,944 |
| outbox renewal | 14 → 11 | 16 → 16 | 4,468 → 3,608 |

Acknowledgement measurements are warm duplicate acknowledgements of one previously claimed item. Renewal replaces the held envelope with the returned one on every call and increases expiry by one tick. The probe verifies final durable acknowledgement state or renewal expiry. These small fixed-record fixtures establish allocation traffic for those paths, not first-ack latency, bulk dispatch throughput or a universal Penelope overhead percentage. Rust System allocation requests/reallocations are counted globally across the submitting and worker threads; SQLite's own C allocator and peak/live memory are not measured. Instrumented allocation runs make no wall-clock speed claim. Temporary databases use the project qualification directory; no tmpfs timing result is inferred.

## Reliability and qualification

The full process suite passes on both the restored baseline and candidate with the same expanded evidence test. The corruption matrix now includes wrong SQLite storage classes and oversized stored lease, payload and checksum BLOBs, in addition to existing checksum, payload identity, expiry, missing expiry, identity and attempt faults. Both acknowledgement ports and outbox renewal reject damaged evidence without completing work. Existing tests continue to cover stale envelope fencing, renewal, cancellation, suppressed writes, batch rollback, retries and reopening.

Stable workspace all-target/all-feature tests excluding the fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting and layer checks pass. Both compilers pass 33 SQLite unit, 54 process and 42 task tests. `graphify update .` also passes. The separate fuzz crate and optional StateChronicle conformance were not freshly executed locally. Debug benches in all-target qualification are functional checks, not controlled performance measurements.

[Evidence and reproduction](verification-data/2026-10-31-lease-validation/README.md) contains the raw allocation samples, qualification logs, baseline/candidate validation source, probe/allocator source, comparison script, summary generator and hash manifest. Plans from Downloads are not included.
