# SQLite cached byte-charge integrity — 2026-10-31

Source: `e6b47d2cffdca20343751d0c4ca14f1bd26b9320`. Baseline: `407759a2a0fffa5a163715ec33cb120c4f1cbc0e`.

The append cache previously matched scope, next sequence and last-record digest
but ignored the stored bytes_used counter. Changing only that counter allowed
a warm connection to acknowledge an append and propagate an inconsistent charge.
Cold reconstruction already rejected the mismatch.

The cache now retains the validated byte charge and compares it with the full
stored head before reusing the projection. A mismatch triggers existing bounded
reconstruction, which rejects an inconsistent charge as Corrupt. Commit and
explicit head rebuild both populate the charge. Coherent-cache appends keep
their existing path without a history scan; the cache adds one u64 value and
comparison. No performance improvement or application overhead measurement is
claimed. No schema or public API changes are introduced.

## Reproduction and recovery

Four public-adapter fixtures cover counters changed to zero and incremented by
one, with the cache populated through commit or rebuild_head. Baseline accepts
all four appends: the fixture-only patch produces four failing tests and four
unexpected Ok(TaskRecord) results (exit 101).

The candidate returns Corrupt. Each fixture verifies the complete stored head
and record count remain unchanged, the rejected sequence is absent, explicit
maintenance rebuild restores the original charge, the exact append then succeeds,
and reopening returns the record and original input blob. Direct SQL mutation
is confined to each test's own temporary database. This proves these head-counter
cases, not authentication against arbitrary malicious database rewriting.
Unchanged immutable history remains trusted on a coherent-cache append.

## Qualification

- Full workspace tests pass on Rust 1.99.0 and Rust 1.85.0, including 18 SQLite
  unit tests and all 26 SQLite task integration tests.
- Strict workspace Clippy, formatting, layer checks and AST graph update pass.
- 25 isolated repetitions of each of four fixtures per compiler give 200
  successful rejection, rollback-state, repair, retry and reopen scenarios.
- The fixture-only patch applies cleanly to baseline with `git apply --check --whitespace=error`.

Tests use empty RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER,
`TMPDIR=/home/ac/projects/penelope/target/qualification-temp`, and
`RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz`.
MSRV uses `cargo +1.85.0` with `CARGO_TARGET_DIR=target/worker-audit-msrv`.
Clippy uses the same workspace/target/features/exclusion flags and `-- -D warnings`.
Other checks: `cargo fmt --all -- --check`, `bash scripts/check-layer-boundaries.sh`,
`graphify update .`. The targeted baseline command is
`cargo test -p penelope-sqlite --test tasks cache_usage::`, run with the same
wrapper/TMPDIR/serial settings; final source passes the four cases.

[Retained evidence](verification-data/2026-10-31-sqlite-cache-usage/evidence.json)
includes baseline and qualification logs, all repeated outputs and binary hashes,
source/artifact hashes, fixture patch and runner. Gzip timestamps are deterministic.
Run the directory's verify.py to independently validate the reproduction and
repetition coverage. The runner uses binaries discovered from the qualification
logs; reproduce after both full suites in a disposable source checkout and
adapt fixed TMPDIR paths for another host.
