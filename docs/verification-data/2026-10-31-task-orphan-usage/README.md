# First-append retained-blob quota evidence

The candidate fix and baseline full commits are recorded in `manifest.json` and [the report](../../verification-task-orphan-usage-2026-10-31.md). Root evidence describes the quota fix. `rejected-aggregate/` describes a separate, rejected SQL optimization measured before the quota bug was found; its candidate source is not production.

Verify without third-party Python packages:

```sh
python3 docs/verification-data/2026-10-31-task-orphan-usage/verify.py
```

This verifies archive hashes, regression/control and qualification logs, source snapshots, process/profile ordering, allocation samples, and all 19 recomputed bootstrap statistics. It does not reproduce hardware timings or independently authenticate logs. Gzip streams have mtime zero. Executables are represented by SHA-256 hashes.

For final correctness reproduction, use a disposable checkout of the candidate fix. Set `RUSTC_WRAPPER=` and `RUSTC_WORKSPACE_WRAPPER=`, and set `TMPDIR` to an existing absolute `target/qualification-temp` directory. Copy root Python scripts into `target/task-orphan-usage`. Run:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
python3 target/task-orphan-usage/repeat.py
python3 target/task-orphan-usage/baseline-control.py
```

The baseline-control script temporarily replaces tasks.rs with pinned baseline `4bb1f8394030600a306e86b6c5bffaae550da54d`, requires the expected over-quota acceptance failure, restores fixed source in `finally`, then reruns complete SQLite unit/process/task suites on stable and MSRV. Run this source mutation separately without concurrent builds. Baseline-control failure is intentional; final logs must pass. It stops at the first old-code failure, whereas each fixed-code repeat completes all 12 combinations. The full workspace/all-target qualification preceded the naming-only test cleanup; complete SQLite suites passed again afterward on both compilers.

To reproduce the rejected measurements, use a separate disposable checkout of the baseline, replace tasks.rs/lib.rs/Cargo.toml with `rejected-aggregate/candidate-*` source snapshots and use the unchanged benchmark fixtures from that baseline. Copy its build/compare/analyze scripts into `target/task-replay-usage`, create `target/qualification-temp`, then run those three scripts in order. The build script compares its pinned baseline to the current experimental source, restores temporary source replacements in `finally`, and uses Cargo JSON artifacts to identify executables. Run measurements without concurrent project builds/tests, profiling, graph updates or bootstrap analysis. Linux CPU 0 affinity and `taskset` are required. Profile flags are explicitly cleared/set; scripts overwrite target results. Allocations count Rust allocator calls, not SQLite C allocations or total memory.

Final local qualification excludes the separate fuzz crate and optional StateChronicle gate. Prior CI success is identified separately and does not establish current-code CI. The combined-query candidate was rejected before full qualification. No release plan from Downloads is included.
