# Claim encoding buffer reuse evidence

Full candidate/baseline pins are in `manifest.json` and [the report](../../performance-sqlite-claim-encoding-2026-10-31.md). Verify using Python's standard library:

```sh
python3 docs/verification-data/2026-10-31-claim-encode-reuse/verify.py
```

The verifier checks artifact/source hashes, baseline compatibility and missing-clear mutation controls, 40 repeated serialization/integration processes, full qualification, 192 ordinary latency processes and their 12 recomputed comparisons, and all 512 allocation samples. It does not independently authenticate logs or reproduce hardware timing. Gzip streams use mtime zero. Six executable hashes identify four ordinary latency and two instrumented allocation binaries; executables are not included in Git.

Reproduce in a disposable candidate checkout. Create `target/claim-encode-reuse`, its `confirmation` subdirectory and `target/qualification-temp`. Copy root Python scripts into the former and confirmation scripts into the subdirectory. Clear compiler wrappers and set TMPDIR to an absolute writable build directory when running Cargo.

```sh
python3 target/claim-encode-reuse/controls.py
python3 target/claim-encode-reuse/build.py
TMPDIR=/tmp python3 target/claim-encode-reuse/capture-storage.py target/claim-encode-reuse/storage.json
TMPDIR=/tmp python3 target/claim-encode-reuse/compare.py
TMPDIR=/tmp python3 target/claim-encode-reuse/allocations.py
TMPDIR=/tmp python3 target/claim-encode-reuse/capture-storage.py target/claim-encode-reuse/confirmation/storage.json
TMPDIR=/tmp python3 target/claim-encode-reuse/confirmation/compare.py
python3 target/claim-encode-reuse/analyze.py
python3 target/claim-encode-reuse/confirmation/analyze.py
python3 target/claim-encode-reuse/analyze-allocations.py
```

Controls temporarily replace both process.rs and leases.rs with the baseline, then remove the candidate buffer clear, always restoring originals in `finally`. Baseline integration compatibility must pass; the missing-clear control must fail at the first outbox stored-byte comparison. It stops there and does not independently exercise the mutant timer case. Restored candidate unit/integration tests must pass. The build script swaps both source files between variants, injects allocation instrumentation into lease_batch.rs only for the separate allocation binaries, identifies exact Cargo executables from JSON artifact messages, and restores all originals in `finally`. Do not run concurrent builds/tests while source is temporarily replaced.

No other project builds/tests, graph updates, allocator probes or bootstrap analyses should run during either ordinary timing campaign. Allocation samples run separately and their instrumented elapsed times are ignored. Counting covers Rust allocation/reallocation traffic and requested bytes across the caller's public call and worker operation; it stops before result checking/destruction and probe output. It excludes SQLite C allocation, deallocation, live/peak memory and RSS. All fixture result, attempt, unique-token and persisted-counter assertions remain enabled. Instrumentation uses the existing System allocator/atomic pattern and is archived rather than added to production.

Scripts require Linux `taskset` CPU 0 and overwrite output files. Here `/tmp` is tmpfs; WAL/FULL settings do not imply device synchronization on tmpfs. Each campaign uses the same four ordinary executables with 24 AB/BA pairs. Empty observations contain 2,000 calls; nonempty observations contain 400 expired claims across eight fixtures after warmup. Every observation is retained. Device/cache state and unrelated system activity are not controlled; no device-backed throughput or global Penelope overhead claim follows.

For final qualification, clear RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER and set TMPDIR to the absolute qualification-temp directory:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
python3 target/claim-encode-reuse/repeat.py
```

The instrumented benchmark was also installed temporarily and checked with strict Clippy, then restored before final qualification. `previous-ci.json` records baseline CI success, including bounded fuzzing and optional StateChronicle conformance, rather than fresh local runs of those separate gates. The release plans in Downloads are not included.
