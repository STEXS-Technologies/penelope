# Duplicate inbox admission qualification

Verify with `python3 docs/verification-data/2026-10-31-inbox-validation/verify.py`. This checks source/archive hashes, all 512 allocation samples, 192 timing profile rows from 96 processes, regenerated timing/bootstrap summaries, the intentionally failed checksum-bypass control, 20 repeated regression processes and final qualification logs. It validates archived evidence rather than independently authenticating logs or establishing global application overhead.

Use a disposable candidate checkout to reproduce. Create target/inbox-validation and target/qualification-temp. Copy compare.py, control.py, timing.py, timing_summary.py, repeat.py and summarize.py into the former; decompress baseline-process.rs.gz there. Install stable Rust, Rust 1.85.0 and taskset. Clear RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER and set TMPDIR to the absolute project qualification-temp directory. Run:

```sh
python3 target/inbox-validation/compare.py
python3 target/inbox-validation/control.py
python3 target/inbox-validation/timing.py
python3 target/inbox-validation/timing_summary.py
python3 target/inbox-validation/summarize.py
python3 target/inbox-validation/repeat.py
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
```

compare.py temporarily installs the pinned baseline process.rs, runs the full process suite and builds/copies both ordinary and allocation probes, then repeats for the candidate. It always restores candidate source in finally. control.py intentionally bypasses the byte-match checksum check, requires the corresponding first failed assertion, restores source and reruns both new tests. Do not run other builds/tests during source replacement. The failed control log is intentional; all restored/final checks must pass.

Run timing.py after both scripts finish, without concurrent builds, tests, graph updates, bootstrap analysis or allocation probes. It chooses the lowest CPU allowed by affinity (CPU 0 in this run), pins both caller and worker, and uses project qualification-temp storage. It runs two campaigns of 12 ABBA/BAAB four-process blocks, with 64 warmup and 5,000 measured admissions per pending/consumed profile in every process. No samples are dropped. timing_summary.py compares per-block geometric means and bootstraps 12 blocks 20,000 times with deterministic seeds. Confidence intervals are scoped to this workload and machine; CPU frequency/background load are uncontrolled. The ordinary executable does not install the counting allocator.

The same typed fixture drives allocation probes: four processes per revision, two profiles, 64 warmups and 32 measured samples each. Counts include Rust System requests across submitting/worker threads, excluding SQLite C allocations and peak memory. No inference about first admission or fallback-path latency is made.

Twenty regression processes cover the two tests: pending/consumed compatibility, conflicts and corruption with warm/reopened workers and exact ten-table snapshots, plus exact/oversized stored-record limits. Fixture counts total 480. The baseline passes this same preservation coverage. Mutation controls stop at their first expected failure.
