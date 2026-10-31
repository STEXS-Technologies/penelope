# Exact commit validation qualification

Verify with `python3 docs/verification-data/2026-10-31-commit-validation/verify.py`. It checks archive/source hashes, all 512 allocation samples, the intentionally failed checksum-bypass control, 20 repeated integrity processes and final qualification logs. It checks recorded evidence, not independent log authenticity or latency.

To reproduce, use a disposable candidate checkout with stable Rust and 1.85.0. Create target/commit-validation and target/qualification-temp. Copy compare.py, control.py, repeat.py and summarize.py into the former; decompress baseline-process.rs.gz there. Clear RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER, and set TMPDIR to the absolute project qualification-temp directory. Run:

```sh
python3 target/commit-validation/compare.py
python3 target/commit-validation/control.py
python3 target/commit-validation/summarize.py
python3 target/commit-validation/repeat.py
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
```

compare.py temporarily installs baseline process.rs from the pinned baseline, runs the process suite and builds/copies/runs the allocation probe; it repeats for candidate source and restores it in finally. control.py intentionally bypasses outcome checksum validation, requires the expected integrity test failure, restores source and reruns that test. Do not run concurrent builds/tests while either script replaces source. Failed control logs are intentional; restored/final qualification must pass.

The probe runs typed public commits and warmed exact retries, validates each receipt and asserts unchanged durable constituent counts. Each revision has four processes with two fixtures and 32 samples per fixture after 16 warmups. Rust System requests are counted across caller/worker threads; SQLite C allocations, peak memory, first-commit latency and global overhead are outside this measurement. Results can differ with toolchain/dependency changes.

The expanded integrity test warms verification, damages the later outcome/action or input/head, and requires unchanged ten-table snapshots after rejected warm and reopened retries. Twenty repeated processes run 460 fault fixtures and 920 rejected retries. The unchanged baseline passes this same regression, while the intentional checksum-bypass control stops at its first detected failure.
