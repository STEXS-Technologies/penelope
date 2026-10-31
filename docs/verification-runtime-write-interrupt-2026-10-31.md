# Interrupted runtime writes — 2026-10-31

Source: `ce36ba723ec4dda20f00255723f02882abd1b6d6`. Baseline: `fecc2faebfe6a42137fb1590115f7884ccecb872`.

The shared startup-frame/checkpoint-ACK write helper now retries Interrupted
immediately under its original deadline. Previously it treated Interrupted like
WouldBlock and slept one millisecond on every retry. Interruption does not
establish pipe backpressure. WouldBlock retains its one-millisecond backoff;
positive-write expiry checks and the single transfer deadline remain intact.

## Reproduction and evidence

Three new unit fixtures run the actual private production helper:

- Sixteen injected interruptions interleaved with four positive short writes,
  crossing two frame parts, preserve all bytes without duplication.
- Eight injected WouldBlock errors with the same short-write/part boundaries
  preserve all bytes and serve as the backpressure control.
- A continuously interrupted writer exhausts its two-millisecond deadline;
  reusing the expired deadline performs no additional write attempts.

Five isolated runs per case, variant and compiler give 60 traced executions.
Both Rust 1.99.0 and 1.85.0 show exactly sixteen sleep syscalls on the baseline
interruption fixture and zero on the candidate. The WouldBlock fixture shows
eight sleep syscalls on both variants. The continuous interruption fixture
successfully expires with positive attempted-write counts; the candidate makes
zero sleep syscalls there too. Tests themselves assert byte integrity, attempt
counts for finite bursts, expiry, and no writes after expiry. They do not assert
elapsed-time thresholds or require a particular scheduler speed.

The writer errors are synthetic fault injection, not delivered OS signals.
The syscall traces observe real nanosleep/clock_nanosleep calls from the compiled
library tests. This directly proves removal of retry sleeps in these cases;
it does not measure application overhead or a typical-workload speedup. Persistent
interruptions can now retry continuously until the existing deadline expires.

`trace.py` builds baseline and candidate with the same new fixture file,
substituting only runtime.rs and restoring it in a finally block. It records
all commands, return codes, output, compiler/build records, source and binary
hashes. Each process has a ten-second outer guard. Strace uses
`-f -qq -e trace=clock_nanosleep,nanosleep`; targeted libtest invocations select
one exact case with `--nocapture --test-threads=1`. There are no signal delivery
or wall-clock performance claims. The fixture patch applies cleanly to baseline.

## Qualification

All commands use empty RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER. Tests use
`TMPDIR=/home/ac/projects/penelope/target/qualification-temp`.

- `RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz` passes.
- The same full suite using Rust 1.85.0 and `CARGO_TARGET_DIR=target/worker-audit-msrv` passes.
- Both suites include 28 runtime unit tests and 66 runtime integration tests,
  including real startup timeout/recovery, checkpoint, fencing and sandbox cases.
- `cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings` passes.
- `cargo fmt --all -- --check`, `bash scripts/check-layer-boundaries.sh`, and `graphify update .` pass.

[Evidence manifest](verification-data/2026-10-31-runtime-write-interrupt/evidence.json)
contains source and artifact hashes. All trace text, process outputs, build and
qualification logs, the fixture-only patch and runner are retained. Gzip timestamps
are deterministic. `verify.py` independently checks every sleep count, successful
case output, hashes, and both workspace logs. Reproduction requires Linux strace
and the two toolchains; run trace.py in a disposable source checkout with a fresh
output directory and adjust the fixed TMPDIR path for a different host.
