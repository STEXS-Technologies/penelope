# Worker transaction panic boundaries — 2026-10-31

Added a disk-backed WAL regression test for the SQLite database worker. Runtime behavior is unchanged: only a `cfg(test)` module declaration and its tests were added. Earlier worker panic tests used autocommit writes; this test distinguishes transaction rollback from a commit whose acknowledgement is lost.

Each of eight scenarios gates the first operation after inserting a row inside a transaction, then admits a second operation into the capacity-one queue. The first response is either retained or canceled; admission is either retained or closed by dropping the worker handle. On release, the first operation either panics before commit or commits and then panics.

The queued operation must open a fresh transaction, insert its own row and commit successfully. The exact sorted history must be `[2]` after a pre-commit panic and `[1, 2]` after a post-commit panic. A separate newly opened connection must see the same history and pass `PRAGMA integrity_check`. Every retained first response must resolve to `PortError::Unavailable`. When admission stays open, another accepted read must also return the same history. These checks cover rollback cleanup, committed-write preservation, cancellation, queue drainage after dropping the worker handle, typed completion and subsequent worker reuse.

The eight combinations run in twenty isolated executions: ten stable and ten Rust 1.85.0. All 160 gated scenarios passed, split equally between pre-commit and post-commit panic histories. Both complete SQLite unit suites passed all 34 tests. Strict workspace Clippy, formatting and layer checks passed, and the local graph was updated. This is orderly in-process panic/cancellation coverage, not process-kill or power-loss testing. The second connection can open while the worker's SQLite connection is still alive.

## Fault control

Temporarily bypassed operation-level panic translation in `DatabaseWorker::run` while preserving the outer worker-thread panic boundary. The first callback unwound and its transaction rolled back; the queued transaction completed, but the original accepted response never received a result. The new test failed with `TimedOut` instead of the required `Unavailable`. Production worker source was restored before successful qualification and repeated runs. This control proves detection of unresolved accepted responses; it does not independently prove every rollback or durability assertion.

[Retained evidence](verification-data/2026-10-31-worker-transactions/) contains all repeated scenario logs, stable/MSRV unit-suite logs, strict Clippy output, the exact control and source hashes. Run `python3 docs/verification-data/2026-10-31-worker-transactions/verify.py` to check the retained results. No performance improvement is claimed.
