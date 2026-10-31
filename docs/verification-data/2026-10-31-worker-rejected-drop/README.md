# Rejected-request cleanup containment evidence

Candidate and baseline full commits are in `manifest.json` and [the report](../../verification-worker-rejected-drop-2026-10-31.md).

Verify using Python's standard library:

```sh
python3 docs/verification-data/2026-10-31-worker-rejected-drop/verify.py
```

The verifier checks archive hashes, the four old-code rejected-request unwind cases, 20 repeated worker-suite logs, final qualification, source identity, benchmark process order/profile shape and all six recomputed bootstrap statistics. It does not reproduce hardware timing or independently authenticate logs. Gzip streams use mtime zero; binary payloads are represented by SHA-256 hashes.

Use a disposable checkout of the candidate to reproduce. Create `target/worker-rejected-drop` and `target/qualification-temp`, and copy root Python scripts into the former. Set empty `RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER`, and set `TMPDIR` to the absolute qualification-temp directory. For measurements:

```sh
python3 target/worker-rejected-drop/build.py
python3 target/worker-rejected-drop/compare.py
python3 target/worker-rejected-drop/analyze.py
```

The build script temporarily replaces worker.rs with the pinned baseline or current candidate, identifies exact executable paths from Cargo JSON artifacts, and restores source in `finally`. Benchmark fixtures are unchanged between variants and archived. Do not run concurrent builds while source is temporarily replaced. Measurement requires Linux `taskset` CPU 0 affinity and must run without concurrent project builds/tests, profiling, graph updates or bootstrap analysis. Scripts overwrite target outputs. Latencies include worker dispatch and SQLite transactions; nonempty batch size is claims per request, not separate worker jobs.

Correctness qualification:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
python3 target/worker-rejected-drop/repeat.py
python3 target/worker-rejected-drop/baseline-control.py
```

The baseline-control script deliberately runs the candidate panic regression module against only the pinned old worker.rs, disables core dumps and requires both rejection tests to fail after all four queue-full/disconnected × ordinary/nested modes unwind. Its caller observation boundary safely contains old-code payload cleanup. It restores fixed worker.rs in `finally` and reruns all SQLite unit/process/task suites on both compilers. Run it without concurrent builds. The failed baseline log is intentional; final logs must pass.

Twenty repeated worker suites cover 80 rejected-callback scenarios and 160 existing canceled-result scenarios. Channels gate active work and queue admission without sleeps. In-memory SQLite writes establish visibility and queue progress, not independent crash-durability proof. The secondary-panic-payload retention policy is the existing containment helper's exceptional behavior. Catching unwinding cleanup does not recover aborting processes or indefinitely blocking destructors.

Current local qualification excludes the separate fuzz crate and optional StateChronicle gate. `previous-ci.json` records their success for the baseline only. No release plan from Downloads is included.
