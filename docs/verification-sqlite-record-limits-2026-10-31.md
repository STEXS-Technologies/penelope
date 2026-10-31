# SQLite commit admission with large record limits — 2026-10-31

Raising `SqliteLimits::max_record_bytes` above 16,647,160 previously made even a
small valid saga commit return `QuotaExceeded`. The store opened successfully,
but the commit rejected an overflow in its aggregate encoding allowance before
checking the actual bytes. The allowance now saturates at `u32::MAX`, preserving
admission for the same small records when the configured limit increases.

## Source and behavior

Baseline: `4ae1f39ec6d67212615072e8343be0a8cb83f55f`.
Candidate: `98b0fe3`.

The aggregate allowance multiplies the record limit by 258: the public bounds
of 128 outcomes and 128 actions, plus two. A checked `u32` multiplication failed
for every record limit above `floor(u32::MAX / 258)`, regardless of the actual
batch size or serialized length. These larger record limits were admitted by
`SqliteProcessStore::open`, so increasing an allowance could disable otherwise
valid writes.

`commit_process` now uses saturating multiplication for this aggregate ceiling.
It does not reserve the ceiling's worth of memory. Existing `encode` still
compares the actual serialized length with the ceiling and separately checks
each input, outcome and action against the configured per-record limit. Scope,
history, database-page, transaction and required-row-change checks remain in
their existing paths. There is no schema migration, public API, wire encoding,
digest, or default-limit change. No latency improvement is claimed.

This fixes admission for small batches under large limits; it does not promise
support for an aggregate JSON encoding larger than `u32::MAX`, or make a large
configured record limit a safe memory budget for an arbitrary application.
Serialization still creates the actual JSON buffer before checking its length.

## Public regression and reproduction

The test `record_limits::raising_record_limit_preserves_commit_retry_replay_and_reopen`
uses a fresh WAL/FULL database for each of five record limits:

| Record limit | Baseline first commit | Candidate first commit |
| ---: | --- | --- |
| 4,096 | Success | Success |
| 16,384 (default) | Success | Success |
| 16,647,160 (multiplication boundary) | Success | Success |
| 16,647,161 | `QuotaExceeded` | Success |
| 4,294,967,295 (`u32::MAX`) | `QuotaExceeded` | Success |

The identical batch contains one input, one outcome and one outgoing action.
For each successful configuration, the fixture checks the initial receipt,
exact retry, outcome replay, exact claimed action, acknowledgement, reopening,
a second exact retry and replay, empty acknowledged outbox and the exact token
counter of 2. On a failed initial commit, it verifies the process tables and
token counter remain identical to the pre-call snapshot. The baseline therefore
failed two configurations without leaving partial writes; this was a false
quota rejection rather than an atomicity failure.

The new test failed against unchanged baseline production code and passed after
the arithmetic fix. It also passed in **50 isolated processes**, 25 each on Rust
1.99.0 and 1.85.0, covering **250 limit configurations** through commit, retry,
replay, acknowledgement and reopen. Each subprocess has a 30-second guard; the
public future helper retains its own deadline.

## Qualification and evidence

Passed with compiler wrappers cleared:

- Full workspace all-target/all-feature suites excluding `penelope-fuzz`, with
  `RUST_TEST_THREADS=1`, on Rust 1.99.0 and 1.85.0.
- Workspace Clippy, all targets/features, with `-D warnings`.
- Formatting, layer boundaries, and `graphify update .`.

Each compiler passed 23 SQLite unit, 40 process integration and 26 task
integration tests. Existing tests retain quota exhaustion, exact retry integrity,
suppressed writes, lease eligibility, cancellation and crash/recovery coverage.
This is correctness qualification, not a new performance campaign.

[Evidence directory](verification-data/2026-10-31-sqlite-record-limits) retains
the failing baseline and fixed outputs, baseline/candidate process sources,
public fixture/module registration, source patch, compiler metadata, all isolated
outputs and binary hashes, full-suite/check logs, and checksums. Executables are
not stored in Git. To reproduce the baseline, add the archived test fixture and
registration to an isolated checkout of the pinned baseline and run:

```sh
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo test -p penelope-sqlite --test process record_limits:: -- --nocapture --test-threads=1
```

Verify all recorded artifacts, boundary outcomes, complete compiler/repetition
coverage and SQLite suite counts:

```sh
python3 docs/verification-data/2026-10-31-sqlite-record-limits/verify.py
```
