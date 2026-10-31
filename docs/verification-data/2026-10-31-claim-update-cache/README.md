# Claim UPDATE statement reuse evidence

Full candidate/baseline pins are in `manifest.json` and [the report](../../performance-sqlite-claim-updates-2026-10-31.md). Verify using Python's standard library:

```sh
python3 docs/verification-data/2026-10-31-claim-update-cache/verify.py
```

This checks artifact hashes, source identity, baseline compatibility, 20 repeated expanded rollback tests, final stable/MSRV SQLite and workspace qualification, complete benchmark pairs/profile shapes and all 18 recomputed bootstrap statistics. It does not independently authenticate logs or reproduce hardware timing. Gzip streams use mtime zero; benchmark executables are represented by SHA-256 hashes.

Reproduce in a disposable checkout of the candidate. Create `target/claim-update-cache`, `target/claim-update-cache/disk`, `target/claim-update-cache/disk-repeat` and `target/qualification-temp`, copy root Python scripts into the former and the two disk scripts into its disk and disk-repeat subdirectories. Clear `RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER`; use an absolute writable TMPDIR for builds. Run:

```sh
python3 target/claim-update-cache/build.py
```

The script temporarily replaces leases.rs with the pinned baseline or current candidate, extracts exact executables from Cargo JSON and restores source in `finally`. Do not run concurrent builds while source is replaced. All three campaigns run the same four executables. With no concurrent project builds/tests, profiling, graph updates or bootstrap analyses:

```sh
TMPDIR=/tmp python3 target/claim-update-cache/capture-storage.py target/claim-update-cache/storage.json
TMPDIR=/tmp python3 target/claim-update-cache/compare.py
TMPDIR=/home/ac/projects/penelope/target/qualification-temp python3 target/claim-update-cache/capture-storage.py target/claim-update-cache/disk/storage.json
TMPDIR=/home/ac/projects/penelope/target/qualification-temp python3 target/claim-update-cache/disk/compare.py
TMPDIR=/home/ac/projects/penelope/target/qualification-temp python3 target/claim-update-cache/capture-storage.py target/claim-update-cache/disk-repeat/storage.json
TMPDIR=/home/ac/projects/penelope/target/qualification-temp python3 target/claim-update-cache/disk-repeat/compare.py
python3 target/claim-update-cache/analyze.py
python3 target/claim-update-cache/disk/analyze.py
python3 target/claim-update-cache/disk-repeat/analyze.py
```

Replace the absolute path for your checkout and confirm its filesystem. Here `/tmp` is tmpfs and the project directory is NVMe-backed btrfs. All use the adapter's WAL/FULL configuration, but tmpfs does not represent durable device synchronization. Scripts require Linux `taskset` CPU 0 and overwrite their target outputs. Storage/device/cache state and unrelated system activity are not controlled. Benchmark setup is untimed. Empty observations contain 2,000 public claims, nonempty observations contain 400 claims across eight fixtures after warmup. Batch size is leases per claim request. Each campaign has 24 AB/BA pairs and retains every observation.

Correctness qualification, with wrappers cleared and TMPDIR set to the absolute qualification-temp directory:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
python3 target/claim-update-cache/repeat.py
python3 target/claim-update-cache/compatibility.py
```

The compatibility script deliberately runs the expanded rollback fixture against only the baseline leases.rs, restores candidate source in `finally`, then reruns complete SQLite unit/process/task suites on both compilers. Run without concurrent builds. Baseline passing is expected: this is a performance change preserving existing behavior, not a claim that baseline fails a new correctness requirement.

The expanded fixture warms claim statements, installs external triggers suppressing either the second token increment or second lease update, requires unchanged lease rows/counter after each failed batch, removes each trigger, and checks exact tokens after retry and reopen. It covers outbox and timer cases. Its source and the unchanged benchmark fixtures are archived. Twenty isolated runs (ten per compiler) execute 80 fault/rollback scenarios and 40 retry/reopen workflows. These do not by themselves prove crash durability; existing qualification retains separate recovery tests.

`previous-ci.json` records baseline CI success, including bounded fuzzing and optional StateChronicle conformance. Local qualification does not claim fresh runs of those separate gates. The release plans in Downloads are not included.
