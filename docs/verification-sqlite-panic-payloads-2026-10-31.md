# SQLite worker panic-payload containment — 2026-10-31

A caught panic could still terminate the SQLite worker or abort the hosting
process when its payload panicked during destruction. The worker now disposes
of caught payloads inside a second unwind boundary. It preserves accepted queued
work and an already published successful result after a failing wake callback.

## Reproduction and change

Baseline: `ffd6838207299a4b67c83e6261881e3bbdfb92bc`.
Candidate: `8dad8ed`.

Previously, operation handling called `catch_unwind(...).unwrap_or(...)`, which
dropped an error's boxed panic payload before publishing `Unavailable`.
Notification handling stored the `catch_unwind` result, then implicitly dropped
its payload at the end of the block. Both payload destructors could run arbitrary
application code outside the intended containment boundary.

Two new deterministic fixtures inject a payload whose destructor increments a
counter and panics with another payload. That secondary payload also panics if
dropped. The unmodified worker production code aborted in both operation and
notification reproductions: Cargo returned 101 and reported child `SIGABRT`,
with `fatal runtime error: thread result panicked on drop, aborting`.
The baseline worker snapshot contains only the new test-module declaration in
addition to the pinned source. Tests use the real worker and an in-memory SQLite
connection, not a simulated worker loop.

The shared private `contain_panic` helper catches the operation or wake panic,
then attempts to drop its payload within another `catch_unwind`. If that drop
panics, it deliberately **leaks the secondary boxed payload** with `mem::forget`.
Recursively destroying arbitrary panicking payloads has no safe finite stopping
point. The normal success path and ordinary caught payloads are not leaked.
Repeated pathological payload-destructor panics can accumulate retained memory;
this trades that exceptional leak for continued service rather than process
abort. The tests use a zero-sized secondary payload and do not measure a memory
bound for arbitrary application payloads.

Operations still map a caught panic to `PortError::Unavailable`. Wake failures
still occur after publishing/unlocking the result, so they do not replace an
already committed success. There is no schema, public API, queue capacity,
transaction, retry, or durability-policy change. This is a reliability fix;
no new latency or application-overhead improvement is claimed.

## Regression coverage

Each fixture covers four combinations: admission open/closed and first response
retained/cancelled. Channels hold the first operation until its response has
registered a waker and a second operation has entered the capacity-one queue.
There is no sleep-based assumption that a request has been accepted.

The first operation inserts row 1. The second inserts row 2 and must observe
exactly two rows; it cannot complete unless the worker survived the first
operation's panic handling/notification. The fixture also checks that the primary
payload was destroyed exactly once. A retained first response must return
`Unavailable` for the operation panic, or its original `Ok(1)` for the wake panic.
When admission remains open, a further request must return `Ok(3)`.
Five-second test deadlines detect stranded responses; repeated execution is
additionally guarded by a 30-second subprocess timeout.

The operation-panic fixture intentionally inserts outside an explicit transaction
before raising the panic. Its `Unavailable` therefore does not imply that no
write happened. Callers still need authoritative reconstruction after ambiguous
failures; this change does not turn every failed operation into a rollback.

Both fixtures passed in **100 isolated processes**, 25 runs of each test on
Rust 1.99.0 and Rust 1.85.0: **400 scenarios** covering panic origin, cancellation
and admission state. Existing tests retain ordinary panic containment, queue
exhaustion, cancellation after commit, waker clone/drop reentrancy, registration
replacement, and completion races.

## Qualification and limits

Passed with compiler wrappers cleared:

- Full workspace all-target/all-feature tests excluding `penelope-fuzz`, with
  `RUST_TEST_THREADS=1`, on Rust 1.99.0 and 1.85.0.
- Workspace Clippy, all targets/features, with `-D warnings`.
- Formatting, layer boundaries, and `graphify update .`.

Each compiler passed 23 SQLite unit, 39 process integration, and 26 task
integration tests. Existing allocation checks also passed; their debug-suite
outputs do not establish a latency effect. Baseline CI run `41163384186` passed;
candidate CI is tracked separately after push.

Containment requires Rust unwinding. It cannot recover from `panic=abort`,
process exit, a panic hook that aborts, or an abort caused by a second panic
while the original operation is already unwinding. This change addresses payload
destruction after `catch_unwind` has returned, not arbitrary fatal process faults.

## Evidence

[Evidence directory](verification-data/2026-10-31-sqlite-panic-payloads) retains
both baseline abort logs, exact baseline/candidate worker and fixture sources,
source diff, compiler metadata, repeated outputs/binary hashes, full check logs,
and checksums. Executables are not committed. The archived baseline can be
reproduced by replacing `worker.rs` and adding `worker_panic_tests.rs` in an
isolated checkout of the baseline; each failing reproduction should run as its
own subprocess because it deliberately aborts.

```sh
python3 docs/verification-data/2026-10-31-sqlite-panic-payloads/verify.py
```

The verifier checks hashes, both observed baseline aborts, complete compiler/test
repetition coverage, per-process success, and SQLite suite counts. It verifies
recorded evidence, not a new execution of the aborting baseline.
