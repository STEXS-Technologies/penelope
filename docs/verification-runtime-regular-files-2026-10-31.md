# Runtime regular-file verification — 2026-10-31

Source: `a7ecf96d7e63874568131ad7f50b3a39803fd72d`. Baseline: `212c816f49832f0ee11f03a6c6c6cc8fd3903965`.

Linux worker images, saved execution manifests and registry lock files now use
nonblocking opens followed by regular-file checks on the opened descriptor.
This prevents FIFO peer waits and FIFO reads; descriptor checks avoid a separate
path-check/open race. Open/stat failures remain Runtime errors; opened
non-regular files return Corrupt. Registry symlink restrictions remain in place.
Worker symlinks and `/proc/self/fd` sources pointing to regular files remain valid.

## Reproduction and recovery

Applying `baseline-fixtures.patch` to the baseline and running the filtered
runtime integration suite produces 3 passes and 6 failures (exit 101): four
contained five-second FIFO timeouts, an accepted FIFO lock and the directory
image error classification. A FIFO keeper holds a writer open in two cases;
the other two have no writer. The final source passes all nine filtered tests,
including the helper's no-op invocation.

Fixtures repair rejected image paths and reopen successfully. Manifest fixtures
restore the saved manifest and fence successfully while checking the old
generation remains blocked. The lock fixture repairs the lock and fences,
checking rejection occurred before a manifest was created. Test subprocesses
have a five-second guard so regressions cannot indefinitely block the suite.

## Validation

All commands ran with empty RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER;
TMPDIR was `/home/ac/projects/penelope/target/qualification-temp`.

- Rust 1.99.0: `RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz` — exit 0.
- Rust 1.85.0: the same command with `cargo +1.85.0` and `CARGO_TARGET_DIR=target/worker-audit-msrv` — exit 0.
- Both suites include 21 runtime unit tests and 66 runtime integration tests.
- `cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings` — exit 0.
- `cargo fmt --all -- --check`, `bash scripts/check-layer-boundaries.sh`, and `graphify update .` — exit 0.
- `python3 target/repeat-regular-files.py`: eight filesystem cases, 25 repetitions each on both toolchains; all 400 isolated executions pass. The retained runner uses a ten-second outer timeout and records commands, output and executed binary hashes.
- The fixture-only patch applies cleanly to baseline with `git apply --check --whitespace=error`.

## Scope

This proves the tested FIFO rejection and repair behavior. It does not establish
a universal filesystem I/O deadline, bound arbitrary device drivers, or measure
application slowdown. Regular-file opens and metadata operations can still
stall on an unhealthy filesystem. The checks add descriptor metadata calls;
no performance improvement is claimed for this change. Earlier performance
results retain their original source pins.

Evidence: [artifact manifest](verification-data/2026-10-31-runtime-regular-files/evidence.json).
Logs are gzip-compressed with deterministic timestamps. The fixture patch,
repetition runner, complete outputs, source hashes and artifact hashes are retained.
