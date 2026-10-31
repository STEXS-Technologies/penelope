# SQLite outbox lease clock ordering — 2026-10-31

Source: `8a45d9563b2ead09520a3c508c1e4e22542eb21b`. Baseline: `fa8d06746bf67271ad7bf8421102bbd32a287c2f`.

Outbox claim now samples its trusted clock after acquiring the immediate writer
transaction. Previously SQLite lock contention could consume the lease window
between clock sampling and row selection/update. A new claim could return an
already expired lease, or miss reclaiming a lease that expired during the wait.
The single clock callback now runs inside the writer transaction. Subsequent
query/commit time can still consume lease lifetime; this is not a guarantee that
arbitrarily short leases remain valid when returned. No speed or global overhead
measurement is claimed. Timer claims use their existing explicit request time.

## Deterministic reproduction

Fixtures use a real SQLite BEGIN IMMEDIATE on a second connection. The adapter's
worker reaches a test-only SQLite busy handler, which signals actual lock
contention and waits on a bounded channel. The controller advances trusted
logical time from 10 to 100, releases the writer transaction, then releases the
busy callback. No sleep or scheduler-duration assumption decides ordering.
The lease window is 10 logical milliseconds.

- Baseline samples time once while blocked. A new claim returns expiry 20 even
  though time is 100. With a previous lease expiring at 20, reclamation returns
  no lease because it uses stale time 10.
- Candidate samples time zero times while blocked. Both fresh and reclaimed
  claims return one valid lease expiring at 110. Reclamation changes the token;
  another claim cannot steal the active lease, and acknowledgement survives reopen.
- A clock error and expiry overflow both release the writer lock without
  consuming a token. An immediate transaction on another connection succeeds
  with zero busy timeout, and a later valid claim succeeds.

The baseline targeted suite has one passing rollback-control test and two failing
contention tests (exit 101). The candidate passes all three. The fixture-only
patch includes the new unit tests and their module declaration, plus static-only
thread_local inventory support needed by the structural gate; it applies cleanly
to baseline.

## Qualification

Full workspace suites pass on Rust 1.99.0 and 1.85.0, including 21 SQLite unit
tests and 26 SQLite task integration tests. Strict workspace Clippy, formatting,
layer checks and AST graph update pass. Twenty-five isolated repetitions of each
of three tests per compiler give 150 processes: 100 contended claims and 100
clock-error/overflow rollback-and-retry scenarios (the rollback test covers both).

The first full attempt exposed the public-type inventory's unknown-macro guard
rejecting test TLS. The inventory now parses thread_local bodies and permits
only static declarations; a new architecture test also rejects a hidden struct.
Unknown macros retain the existing rejection. That failed attempt, a corrected
test-only SQL integer type, and the corrected architecture binding-name lint are
retained alongside final passing logs.

Tests use empty RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER,
`TMPDIR=/home/ac/projects/penelope/target/qualification-temp`, and
`RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz`.
MSRV uses `cargo +1.85.0` with `CARGO_TARGET_DIR=target/worker-audit-msrv`.
Clippy uses the same workspace/target/features/exclusion flags and `-- -D warnings`.
Other checks: `cargo fmt --all -- --check`, `bash scripts/check-layer-boundaries.sh`,
`graphify update .`. The targeted reproduction command is
`cargo test -p penelope-sqlite --lib lease_clock_tests -- --nocapture` with the
same wrapper/TMPDIR/serial settings.

[Retained evidence](verification-data/2026-10-31-sqlite-lease-clock/evidence.json)
contains all logs, repeated outputs and binary hashes, source/artifact hashes,
the fixture patch and runner. Gzip timestamps are deterministic. Run verify.py
in that directory to independently check baseline effects, repeated outcomes,
qualification logs and hashes. Reproduce after both full suites in a disposable
source checkout, adapting the fixed TMPDIR path for another host.
