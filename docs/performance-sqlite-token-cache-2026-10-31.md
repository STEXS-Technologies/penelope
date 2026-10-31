# SQLite lease-token statement reuse — 2026-10-31

Reusing the lease counter's prepared SELECT and UPDATE reduced nonempty claim
latency in four local batch profiles. These measurements describe expired-lease
reclaiming through the optional SQLite adapter, not overall Penelope application
overhead or throughput for a complete delivery workflow.

## Source and behavior

Baseline: `df5721bd2cb1a396c585f53596697f25b8631371`.
Candidate: `1260dc9`.

`leases::token` now uses `prepare_cached` for its counter read and conditional
increment. The existing bounded, connection-owned rusqlite cache can reuse both
statements across items and claims. It still reads the authoritative counter,
checks the positive value and integer overflow, conditionally increments the
exact previous value, and requires exactly one changed row. Both statements
remain inside the caller's existing immediate transaction. Tokens and counters
are not cached in application memory. There is no schema or public API change.

The baseline already includes the previous optimization that reuses the outbox
and timer selection statements. Only the two token-counter preparation paths
change in this comparison. The new benchmark and rollback fixture accompany the
candidate. Formatting after the timed build wrapped one expression without
changing behavior; the exact compiled source snapshots and their hashes are
retained along with the final source patch.

## Paired measurement

Rust 1.99.0 release binaries, bundled SQLite, AMD Ryzen 9 7950X. Each process
and its worker inherited affinity to the same permitted CPU. Stores are local
temporary WAL/FULL databases. The campaign contains 20 alternating AB/BA pairs,
40 successful processes, with no discarded trials. No other agent builds,
tests, or graph updates ran during timing.

Each process runs outbox and timer profiles at batch sizes 1 and 16. Each
profile uses eight fresh stores containing a real committed batch; timer
profiles schedule every timer before measurement. Each store performs 10
untimed warmup claims and 50 timed claims, advancing logical time by 11 ms for
each claim against a 10 ms lease. Thus each claim reclaims the entire expired
batch while staying below the 64-attempt bound. This is repeated delivery of
existing records, not insertion of new work or an acknowledgement workflow.

The timed span includes the public future, worker dispatch, writer transaction,
selection and integrity checks, token issuance, lease serialization and updates,
commit, and result delivery. Setup, scheduling, request construction, returned
lease destruction, and post-call checks are excluded. Every batch must return
the requested number of leases. The fixture rejects reused tokens, verifies
outbox attempt numbers, and checks the exact persisted counter after every
store's 60 claims.

| Claim | Batch | Baseline mean per batch | Candidate mean per batch | Mean paired latency change | Bootstrap 95% interval |
| --- | ---: | ---: | ---: | ---: | ---: |
| Outbox | 1 | 622.256 µs | 596.721 µs | −3.6043% | [−7.0862%, −0.3853%] |
| Outbox | 16 | 793.836 µs | 739.405 µs | −6.0485% | [−10.7500%, −0.6120%] |
| Timer | 1 | 620.946 µs | 596.248 µs | −3.5433% | [−6.6482%, −0.6167%] |
| Timer | 16 | 784.724 µs | 712.707 µs | −8.6673% | [−11.7111%, −5.6440%] |

Changes average within-pair candidate/baseline ratios; they are not ratios of
the aggregate means in the preceding columns. Intervals bootstrap those 20
ratios 10,000 times with seed 42. All four recorded metrics are reported.
Intervals are individual and descriptive, without multiplicity correction;
three effects have wide intervals approaching zero. Filesystem synchronization
and scheduling noise remain in this end-to-end adapter measurement. Results
do not establish gains for cold starts, other batch sizes, other storage,
other CPUs, competing writers, or Rust 1.85.0 performance.

The campaign covers 1,280 fresh-store fixtures, 64,000 timed and 12,800 warmup
public claims, issuing 544,000 timed and 108,800 warmup leases.

## Durability and correctness checks

The new `warmed_token_statements_preserve_batch_rollback_retry_and_reopen`
integration test runs both outbox and timer cases. It warms the statements by
claiming three actions, then suppresses the counter's second increment during a
reclaim. This occurs after one token and one lease update have succeeded.

The claim returns `Invariant`; all previous lease bytes, attempts, expiries,
and the counter must remain unchanged. After removing the fault trigger, retry
must issue precisely tokens 4–6 with attempt 2 and expiry 121. Reopening and
reclaiming must issue tokens 7–9 with attempt 3 and expiry 132. The test checks
both full batch lengths and disjoint token sets. This is a compatibility and
regression check for statement reuse; the baseline is not claimed to fail it.

The fixture passed in 50 isolated processes: 25 each on Rust 1.99.0 and 1.85.0,
covering 100 outbox/timer rollback, retry, and reopen cases.

Also passed with compiler wrappers cleared:

- Full workspace all-target/all-feature suites excluding `penelope-fuzz`,
  using `RUST_TEST_THREADS=1`, on stable and Rust 1.85.0.
- Workspace Clippy with all targets/features and `-D warnings`.
- Formatting and layer-boundary checks.
- `graphify update .`.

Each compiler passed 21 SQLite unit, 34 process integration, and 26 task
integration tests. Existing tests retain counter exhaustion, suppressed writes,
lease expiry, stale envelopes, cancellation, corruption, and recovery coverage.
The benchmark also executed successfully in each full-suite debug run; those
runs are not controlled performance measurements.

The previous baseline's CI run `41160240353` passed. Candidate CI is tracked
separately after push.

## Retained evidence

[`verification-data/2026-10-31-sqlite-token-cache`](verification-data/2026-10-31-sqlite-token-cache)
contains build and validation logs, exact compiled sources, benchmark source,
source patch, compiler and CPU metadata, binary hashes, all paired and isolated
outputs, scripts, and checksums. Executables are identified by hash and are not
stored in Git. Initial successful builds are retained alongside the final
matched-source builds used in the comparison.

Run:

```sh
python3 docs/verification-data/2026-10-31-sqlite-token-cache/verify.py
```

The verifier checks file and compiled-source hashes, complete pairs, benchmark
binary pins, recomputed changes and intervals, isolated test counts, and SQLite
suite counts. Build/compare scripts retain the original local paths; rebuilding
requires applying the candidate change to the pinned baseline before invoking
the build script, whose baseline lookup uses `HEAD`.
