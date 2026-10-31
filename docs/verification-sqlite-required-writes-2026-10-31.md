# SQLite required-write audit — 2026-10-31

The implementation at 8231e0aab8fda2d9c7de576b3a7215016be75e17 rejects successful SQL statements that
failed to change a required row. It covers process commits, quota accounting,
inbox acceptance, payload storage, timer scheduling/cancellation, task history,
head repair, generation reservations/fences and retirement.

SQLite's successful execution result alone does not establish that a write
happened. BEFORE triggers using RAISE(IGNORE) reproduce this distinction. On
baseline c34107fd770e03b71797a22e820d446fe3ebddc7, six regression tests expose
false success in 23 of 27 fault cases. Examples include an acknowledged atomic
commit with its outbox action missing, an execution reservation with no durable
authority row, and successful retirement with only part of its history removed.

## Implementation

A private helper checks SQLite's existing changed-row return value and requires
exactly one affected row. Its callers run within their existing transactions.
A suppressed required write returns TaskPortError::Corrupt or the hosted
PortError::Invariant mapping, and transaction rollback preserves prior state.

| Required-write group | Newly checked SQL sites |
| --- | ---: |
| Process usage, inbox, sources, outcomes, actions, head and commit receipt | 9 |
| Payload insertion and timer scheduling/cancellation | 3 |
| Task head creation/repair and record append | 3 |
| Generation reservation and fence creation/update | 2 |
| Retirement tombstone insertion | 1 |

Retirement deletes a variable number of rows. An additional scoped existence
query checks that task records, blobs and the derived head are all gone before
committing. Partial deletion fails and rolls back, preserving the history and
its durable fencing evidence.

Existing CAS/claim/ACK/renewal changed-row checks retain their error mappings.
The task-blob deduplication insert retains its immutable readback check when
insertion changes no row. Migration DDL and its freshly created counter are
outside the ordinary-write inventory. There is no schema, wire-format, public
API or dependency change.

## Verification

The deterministic matrix checks 27 cases across six tests. Four cases already
failed closed on the baseline; the other 23 reported false success. Every fixed
case requires the expected error and compares complete logical row snapshots
before and after the rejected operation, including quota and lease counters.
After removing the trigger, the same connection must successfully perform the
operation; independent reopened connections verify durable results.

Valid process retries remain idempotent; duplicate task-journal appends retain
their existing Conflict behavior. Tests also cover live timer cancellation,
missing/corrupt head repair, fencing before and after reservation, and retirement
where only one record or blob is suppressed while other rows are deleted.

- Stable and Rust 1.85 workspace all-target/all-feature suites passed, excluding
  the separately run fuzz package. The SQLite portion contains 13 library,
  33 process and 22 task tests, plus two benchmark smoke executables.
- Strict workspace Clippy, formatting and layer checks passed.
- The two existing real SQLite operation-model fuzz targets each completed
  10,000 iterations with seed 11. They exercise normal protocol/reopen behavior;
  the deterministic trigger matrix supplies write-suppression coverage.
- The knowledge graph was updated after code changes.

[Logs, corpus snapshots and source/artifact hashes](verification-data/2026-10-31-sqlite-required-writes/evidence.json)
retain old-source failures, corrected checks and the exact commands/environment.
The process fuzz campaign's initial corpus and both final corpora are retained.
The authority campaign started from an existing corpus that was not separately
snapshotted before execution; its final corpus supports replay of retained
inputs, rather than exact reconstruction of its original seeded mutation path.

The single-row checks add no SQL statements. Retirement adds one existence
query. No timing comparison was run, so no speedup, zero-overhead guarantee or
new application-wide slowdown percentage is claimed. The supported private
single-host WAL/FULL trust model remains: these checks detect suppressed writes,
and do not authenticate a database against arbitrary malicious rewriting.
Historical full-release/package and extended fuzz qualification remain scoped
to their original revisions.
