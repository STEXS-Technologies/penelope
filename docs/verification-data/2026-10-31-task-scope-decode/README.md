# Task replay scope decoder evidence

Candidate code: `7433d8d` (full pin in manifest). Baseline: `bd187069a30b0c3113089c83260c4b961db45ebe`.
See [the report](../../performance-task-scope-decode-2026-10-31.md) for findings, regressions, limits, and qualification scope.

Root data describes the final lazy decoder. `rejected-raw/` and `eager-scope/` preserve independent earlier trials, including their own raw observations, analyses, source snapshots, executable hashes and logs. Do not pool campaigns or attribute prototype test logs to the final source. `initial-*` logs may include development failures; final qualification is `workspace.log.gz`, `msrv.log.gz`, `clippy.log.gz`, and the root `repeat-*` logs. `negative-control.log.gz` intentionally contains a failed test: disabling identity comparison must fail the oracle check. It was executed before final identifier/comment-only cleanup; final production tests were rerun afterward.

Verify without compiling or requiring third-party Python packages:

```sh
python3 docs/verification-data/2026-10-31-task-scope-decode/verify.py
```

This hashes every archived evidence file and recomputes all 78 bootstrap statistics from compressed raw observations in temporary directories. It checks process order/profile shapes, allocation data, the 80 final regression-process logs, negative control, qualification summaries and source/binary provenance. It does not reproduce hardware timing or prove that a log is authentic independently of this archive. Gzip streams use mtime zero. Executables are represented by SHA-256 hashes rather than committed binary payloads.

To reproduce final measurements, use a disposable checkout of the candidate commit and the recorded Rust/compiler environment. From the repository root, create `target/task-scope-decode` and `target/qualification-temp`; copy the root Python scripts and `confirm-provenance.json` from this directory into `target/task-scope-decode`. `build.py` compares the pinned baseline with current candidate sources, using the current benchmark fixtures for both. It temporarily replaces tasks.rs/lib.rs/Cargo.toml and restores them in `finally`; run it only in a disposable checkout with no concurrent builds. Cargo JSON artifact messages identify exact executable paths. Then run:

```sh
python3 target/task-scope-decode/build.py
python3 target/task-scope-decode/compare.py
python3 target/task-scope-decode/analyze.py
python3 target/task-scope-decode/prepare-confirm.py
python3 target/task-scope-decode/confirm.py
python3 target/task-scope-decode/analyze-confirm.py
```

`prepare-confirm.py` similarly rebuilds the pinned historical pre-validation control and restores sources, then prepares the three confirmation executables. The historical control is only for performance comparison. On the original environment its rebuilt executable matched the previously measured binary byte-for-byte (`rebuild-control.json`); different toolchains/paths may yield different hashes. Do not compare newly built artifacts with archived timing as if they had been measured. The original confirmation reused the hash-verified historical executable before that rebuild. Final baseline commit-benchmark identity also matches the prior replay-memo candidate; archived prior manifests preserve provenance.

Run measurement scripts without concurrent project builds/tests, profiling, bootstrap analysis or graph updates. They require Linux CPU 0 affinity (`taskset`) and produce fresh raw logs, overwriting target output files. Flags are cleared/set explicitly for each primary process. The optional pretty fixture modifies raw records and checksums and rebuilds the head outside timing. Alternating checkpoints avoid relying only on memo hits.

To repeat correctness checks, use `repeat.py` (80 isolated processes across stable and 1.85.0). Full final qualification commands, with `RUSTC_WRAPPER=`, `RUSTC_WORKSPACE_WRAPPER=`, and `TMPDIR` set to `target/qualification-temp`, were:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
```

The negative-control script intentionally changes decoder source while running one expected-failure test and restores it in `finally`. Run it separately with no concurrent builds, then rerun production tests. It must never be used as the production decoder.

Prototype reproduction additionally needs the respective archived tasks.rs/lib.rs/Cargo.toml and task_decode.rs/test source snapshots; use a separate disposable candidate checkout and the corresponding frozen build/compare scripts. Intermediate test snapshots and logs retain their original case counts. The final differential suite has 189 directed cases, 189 wrong-checksum controls and 10,000 mutations per run.

The short perf capture (`perf.data.gz`, report/run logs) is exploratory and includes setup; it is not a statistically useful latency attribution. All current checks exclude the separate fuzz crate and optional StateChronicle gate; prior checks of those are not represented as rerun here. No release plan from Downloads is included.
