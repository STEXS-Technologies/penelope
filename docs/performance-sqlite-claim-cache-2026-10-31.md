# SQLite claim statement reuse — 2026-10-31

Repeated empty-queue claims are faster in this local paired benchmark after
reusing the prepared selection statements. This measures SQLite adapter polling;
it does not measure Penelope's overall application slowdown or loaded delivery
throughput.

## Change and source

Baseline: `5c032a202ae91682d1698b54b9dbdc74c95db4d2`.
Candidate: `e812c07`.

In `leases.rs`, the outbox and timer claim SELECT statements now use
`prepare_cached` instead of `prepare`. The connection-owned rusqlite cache is
bounded. Both statements leave scope before transaction commit. Parameters are
bound on each call; SQL, decoding, integrity validation, row-count checks, lease
tokens, expiry rules, and transaction boundaries are unchanged. The benchmark
and Cargo target are included in the candidate commit.

The cached SELECTs avoid repeating SQL compilation on warm polls. This change
does not cache results or omit database reads. Cache misses and eviction still
require preparation.

## Measurement

Release binaries were built with Rust 1.99.0, using bundled SQLite, on an AMD
Ryzen 9 7950X. Each process was pinned to the same permitted CPU. Baseline and
candidate used the same benchmark source before formatting; their production
sources differ only at the two preparation calls. Binary hashes, compiler and
host metadata, source patch, build logs, all process outputs, and the comparison
script are retained in the accompanying evidence directory.

There were 20 pairs in alternating baseline/candidate and candidate/baseline
order, 40 successful processes total. Each fresh temporary WAL/FULL store ran
100 untimed warmup calls followed by 2,000 timed public claims for each queue.
The store contains no actions or timers. Timing includes caller future creation,
worker dispatch, logical clock access where applicable, SQL, empty result
handling, transaction commit, and waking the caller. Store construction is
excluded. Outbox always precedes timer within each process in both variants.
No other agent build, test, or graph job ran during this campaign.

| Public operation | Baseline mean per claim | Candidate mean per claim | Mean paired latency change | Bootstrap 95% interval |
| --- | ---: | ---: | ---: | ---: |
| Empty outbox claim | 16.149 µs | 14.963 µs | −7.3438% | [−7.6446%, −7.0432%] |
| Empty timer claim | 17.581 µs | 17.007 µs | −3.2607% | [−3.6538%, −2.8755%] |

Changes are means of the 20 within-pair candidate/baseline ratios, rather than
a ratio of aggregate means. Intervals resample those ratios 10,000 times using
seed 42. Both recorded metrics are shown; these are descriptive individual
intervals without multiplicity correction. There were no discarded trials.
The two queues account for 160,000 timed and 8,000 warmup public claims.

This is a warm-cache, empty-queue, same-CPU fixture. It establishes no percentage
for full applications, nonempty claims, saturated workers, other hardware,
other compiler versions, or cold startup. Other process-store operations still
prepare their statements normally. The full-suite debug benchmark outputs are
validation runs, not additional controlled performance evidence.

## Validation

Passed with `RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER` cleared:

- `RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz`
- The same full suite on Rust 1.85.0 in a separate target directory.
- `cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings`
- `cargo fmt --all -- --check`
- `bash scripts/check-layer-boundaries.sh`
- `graphify update .`

Each compiler passed 21 SQLite unit, 33 process integration, and 26 task
integration tests. Existing tests exercise nonempty claims, expiry and
reclaiming, stale envelopes, renewal, acknowledgement, cancellation, corruption,
suppressed writes, counter exhaustion, recovery, and transaction rollback.

Evidence is in
[`verification-data/2026-10-31-sqlite-claim-cache`](verification-data/2026-10-31-sqlite-claim-cache).
Run `python3 docs/verification-data/2026-10-31-sqlite-claim-cache/verify.py`
to verify retained hashes, complete pairs, both reported intervals, and the
SQLite integration-suite results. Binaries are identified by hash but are not
stored in the repository.
