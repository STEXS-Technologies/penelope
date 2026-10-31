# Claim row borrowing evidence

Full candidate/baseline pins are in `manifest.json` and [the report](../../performance-sqlite-claim-row-borrows-2026-10-31.md). Verify with Python's standard library:

```sh
python3 docs/verification-data/2026-10-31-claim-row-borrows/verify.py
```

The verifier checks hashes, source identity, two complete latency campaigns and six profiles each, recomputes their bootstrap statistics, recomputes the four allocation profiles from all 512 samples, and checks final qualification and 20 repeated lease-contract logs. It does not independently authenticate logs or reproduce hardware timing. Gzip streams use mtime zero. Executables are represented by SHA-256 hashes and are not included in Git.

Reproduce in a disposable candidate checkout. Create `target/claim-row-borrows`, `target/claim-row-borrows/confirmation` and `target/qualification-temp`. Copy root scripts into the former and confirmation scripts into its confirmation subdirectory. The build script temporarily replaces leases.rs with baseline or candidate source and separately injects allocator instrumentation into lease_batch.rs; it restores both originals in `finally`. It identifies exact Cargo executables from JSON artifact messages. Do not run concurrent builds/tests while these sources are replaced.

```sh
python3 target/claim-row-borrows/build.py
TMPDIR=/tmp python3 target/claim-row-borrows/capture-storage.py target/claim-row-borrows/storage.json
TMPDIR=/tmp python3 target/claim-row-borrows/compare.py
TMPDIR=/tmp python3 target/claim-row-borrows/allocations.py
TMPDIR=/tmp python3 target/claim-row-borrows/capture-storage.py target/claim-row-borrows/confirmation/storage.json
TMPDIR=/tmp python3 target/claim-row-borrows/confirmation/compare.py
python3 target/claim-row-borrows/analyze.py
python3 target/claim-row-borrows/confirmation/analyze.py
python3 target/claim-row-borrows/analyze-allocations.py
```

Do not run other project builds/tests, profiling, graph updates or bootstrap analyses during either timing campaign. Allocation samples are separate processes using separate instrumented binaries; their elapsed times are deliberately ignored. Counting includes both caller and worker across the public call and stops before consuming/destroying the returned value. The existing atomic allocator forwards to System; it does not count SQLite C allocations, live/peak memory or deallocations. All instrumented fixture result and counter assertions remain enabled.

The scripts require Linux `taskset` CPU 0 and overwrite their output files. Here `/tmp` is tmpfs; both campaigns use the same four ordinary release executables. Their WAL/FULL transactions do not represent durable device throughput on tmpfs. Empty observations measure 2,000 claims; nonempty observations measure 400 claims across eight fixtures after per-fixture warmup. Every pair/profile/sample is retained. Other system activity and cache state are not controlled.

For correctness qualification, clear RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER, create the qualification-temp directory and set TMPDIR to its absolute path:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
python3 target/claim-row-borrows/repeat.py
```

The instrumented benchmark was also checked with strict Clippy while temporarily installed as lease_batch.rs, then restored before final qualification. `previous-ci.json` records baseline success including bounded fuzzing and optional StateChronicle conformance. Local qualification does not claim fresh runs of those separate gates. No release plan from Downloads is included.
