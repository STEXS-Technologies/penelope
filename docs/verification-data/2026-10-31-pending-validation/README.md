# Pending-input discovery qualification

Verify with `python3 docs/verification-data/2026-10-31-pending-validation/verify.py`. It checks source/archive hashes, all 768 final allocation samples, 288 timing rows from 96 ordinary processes, regenerated bootstrap summaries, two intentionally failed protection controls, 20 repeated regression processes and final qualification logs. This validates recorded evidence, not independent authenticity or global application overhead.

Use a disposable candidate checkout to reproduce. Create target/pending-validation and target/qualification-temp. Copy compare.py, controls.py, timing.py, timing_summary.py, repeat.py and summarize.py into the former; decompress baseline-inbox.rs.gz there. Install stable Rust, Rust 1.85.0 and taskset. Clear RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER and set TMPDIR to the absolute project qualification-temp directory. Run:

```sh
python3 target/pending-validation/compare.py
python3 target/pending-validation/controls.py
python3 target/pending-validation/timing.py
python3 target/pending-validation/timing_summary.py
python3 target/pending-validation/summarize.py
python3 target/pending-validation/repeat.py
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
```

compare.py temporarily installs pinned baseline inbox.rs, runs the full process suite and builds/copies both release probes, then repeats with candidate source. It always restores candidate source in finally. controls.py intentionally bypasses checksum verification or the record bound, requires the corresponding first failure, restores source and reruns the full process suite. Do not run concurrent builds/tests during source replacement. Failed control logs are intentional; all restored/final checks must pass.

Run timing only after both scripts finish, without concurrent builds, tests, graph updates, allocation probes or bootstrap analysis. It uses the lowest CPU allowed by affinity (CPU 0 here), inherited by the worker, and project qualification storage. Two campaigns of 12 alternating ABBA/BAAB four-process blocks each run all three 0/1/16-input profiles with fixed request limit 16, 64 warmups and 5,000 measured calls. Returned inputs/order and final durable counts are checked. All final-comparison samples are retained. timing_summary.py compares block geometric means and bootstraps 12 blocks 20,000 times with deterministic seeds. CPU frequency/background load remain uncontrolled; results do not transfer automatically to other workloads/machines/core placement. The ordinary executable installs no counting allocator.

The allocation probe shares the typed fixture, with four processes per revision and 32 samples per profile after 64 warmups. Rust System requests across caller/worker threads are counted; SQLite C allocations and peak/live memory are excluded. Empty result allocation is lazy; the table records net public-call costs including statement reuse.

Twenty repeated processes execute three pending-inbox tests each. Integrity fixtures keep a valid first row and damage the later one, including correctly checksummed valid oversized JSON. Both warm/reopened reads must reject with unchanged ten-table snapshots. The refresh lifecycle uses public admission/atomic consumption on another worker and verifies cached-reader/reopened-reader visibility. Existing scope/order/batch bounds are also retained. The baseline passes this preservation coverage. No all-interleavings, power-loss or global overhead claim is made.
