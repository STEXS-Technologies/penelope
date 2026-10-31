# SQLite read error categories — 2026-10-31

Source: `202d500a0bf29de9cc0e0d23845f0bf81c7fc0e7`. Baseline: `9334383800aa84b2ff24bf2d4dd7476b15fa221a`.

Task head, record and blob queries previously mapped every SQLite query error
to Corrupt. They now use the existing shared sql_error mapper: IOERR/BUSY/LOCKED
return Unavailable, FULL/TOOBIG return Exhausted, and CORRUPT/NOTADB remain
Corrupt. Invalid stored types, oversized stored columns, missing required blobs,
checksum failures and malformed records retain their corruption checks. This
change adds no automatic retry policy and makes no performance claim.

## Reproduction and recovery

Three new fixtures use temporary SQL views and a test scalar function to inject
engine errors into the actual public TaskJournal read/read_blob paths. Production
SQL and schema are unchanged. Seven error codes across three read locations give
21 cases. Baseline misclassifies 15; its three matrix tests all fail after each
case's table is restored and same-connection/read-blob/reopen checks pass. The
six corruption cases already pass their expected classification.

On the final source all 21 outcomes match the shared mapper. Removing the
injected fault restores reads on the same connection and after reopening, with
the committed input record and blob intact. This is SQL-engine fault injection;
it does not simulate physical media failure or prove filesystem repair.

## Validation

- Full workspace tests pass on Rust 1.99.0 and Rust 1.85.0, including all 16
  SQLite unit tests and the task/process integration suites.
- Strict workspace Clippy, formatting, layer checks and AST graph update pass.
- Ten repetitions of each of the three matrices per compiler give 60 isolated
  successful processes and 420 injected error/repair/reopen cases.
- The fixture-only patch applies cleanly to baseline with `git apply --check --whitespace=error`.

Tests use empty RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER and
`TMPDIR=/home/ac/projects/penelope/target/qualification-temp`. Full command:
`RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz`.
MSRV uses `cargo +1.85.0` with `CARGO_TARGET_DIR=target/worker-audit-msrv`.
Clippy uses `cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings`.
Other checks: `cargo fmt --all -- --check`, `bash scripts/check-layer-boundaries.sh`,
`graphify update .`. The baseline targeted command filters
`reads_preserve_engine_error_categories_and_recover` with `--lib -- --nocapture`;
it exits 101, while the candidate exits 0.

[Retained evidence](verification-data/2026-10-31-sqlite-read-errors/evidence.json)
includes all logs, process outputs, source/binary hashes, the fixture patch and
repetition runner. Gzip timestamps are deterministic. Run the directory's
`verify.py` to independently validate outcomes, repetition coverage and artifact
hashes. The runner's binary paths come from qualification logs; reproduce in a
disposable source checkout after running both full suites, adapting TMPDIR for
a different host.
