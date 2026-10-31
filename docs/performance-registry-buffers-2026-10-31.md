# Fixed registry buffers — 2026-10-31

Source: `236d897d23152b169a2b0b84b936489393760177`. Baseline: `a0cd454f10a25e85675e44831a658e38354e02a0`.

The Linux registry's fixed 172-byte manifest header now uses a stack array.
Manifest reads use a 173-byte stack buffer, preserving rejection of an extra
byte without a growable vector. Fixed buffers remove allocation from these
encoding/read helpers; the surrounding public operation still allocates.

## Allocation evidence

The existing isolated global Rust allocator probe executes a real protected
worker, commits and acknowledges a checkpoint, receives its completion and
fences it. After that initial fence, it counts each of three repeated public
fences separately. Setup, worker execution and the initial fence are outside
these three counts. The public operation includes authority and registry work.

On both Rust 1.99.0 and Rust 1.85.0, each repeated fence counts **29 Rust
allocations on baseline and 24 on the candidate**. Ten separate process runs
per variant and toolchain give 40 processes and 120 identical-per-variant
fence counts. The probe also retains its zero-allocation assertions for task
polling and checkpoint acknowledgements. C-library allocations are not counted.

The fixture-only patch applies cleanly to baseline. `compare.py` switches only
registry.rs between baseline and candidate, restores it in a finally block,
builds each toolchain's isolated probe, records binary/source hashes and checks
all output and counts. Use it in a disposable checkout of the source commit.
Its target directory and TMPDIR paths reflect this qualification machine.

## Correctness and qualification

- 10,000 deterministic varied headers per toolchain match an independent
  growable-vector legacy wire oracle, including independently encoded scope digests.
- All source lengths 0 through 350 match the legacy bounded reader's acceptance.
- Chunk sizes 1 through 174, six boundary lengths, and alternating Interrupted
  errors exercise short-read collection and retry behavior.
- Injected read errors at each offset 0 through 172 remain Runtime errors,
  including an error while checking EOF after a complete manifest.
- Full workspace tests pass on Rust 1.99.0 and 1.85.0, with 25 runtime unit tests
  and 66 runtime integration tests each. Existing damaged manifest, non-regular
  path, repair, identity and fencing tests remain included.
- Strict workspace Clippy, formatting, layer checks and AST graph update pass.
  The initial Clippy log retains a test lifetime naming failure, corrected before
  final qualification; the final strict Clippy log is clean.

Commands use empty RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER. Tests use
`TMPDIR=/home/ac/projects/penelope/target/qualification-temp` and
`RUST_TEST_THREADS=1`. Full tests:
`cargo test --workspace --all-targets --all-features --exclude penelope-fuzz`;
MSRV uses `cargo +1.85.0` with `CARGO_TARGET_DIR=target/worker-audit-msrv`.
Clippy uses the same workspace/target/features/exclusion flags and `-- -D warnings`.
Other checks: `cargo fmt --all -- --check`,
`bash scripts/check-layer-boundaries.sh`, `graphify update .`.

## Limits

These are Rust allocation counts, not wall-clock speed measurements or an
application slowdown percentage. No end-to-end latency improvement is claimed.
Filesystem synchronization, locking and authority semantics remain intact. The
extra fixed stack storage belongs to the optional Linux host runtime; the
portable task controller and its previously measured stack bound are unchanged.

[Retained evidence and hashes](performance-data/2026-10-31-registry-buffers/evidence.json)
include all qualification logs, comparison build logs, all process outputs,
binary/source hashes, the fixture patch and runner. Logs use deterministic gzip.
