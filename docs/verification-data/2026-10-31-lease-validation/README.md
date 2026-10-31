# Borrowed lease validation qualification

Run `python3 docs/verification-data/2026-10-31-lease-validation/verify.py` to check archive/source hashes, all 768 allocation samples and qualification logs. This validates archived evidence; it does not authenticate the machine or logs independently or measure latency.

To reproduce, use a disposable candidate checkout with Rust stable and 1.85.0 installed. Create `target/lease-validation` and `target/qualification-temp`. Copy `compare.py` and `summarize.py` into the former and decompress `baseline-leases.rs.gz` there. Clear RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER, and set TMPDIR to the absolute project qualification-temp directory. Run:

```sh
python3 target/lease-validation/compare.py
python3 target/lease-validation/summarize.py
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
```

The comparison script temporarily installs baseline validation source, runs the process integration suite, builds/copies the release allocation probe and runs four processes; it repeats with candidate source, restoring the candidate in `finally`. Do not run other builds/tests during this replacement. Allocation results can differ with toolchains or dependencies; the archived summary describes the recorded fixture and pinned revisions. The baseline source is from the full baseline commit in manifest.json; benchmark/Cargo harness and expanded tests are held identical across the comparison.

Each process runs three fresh fixtures with 16 warmup and 32 sampled public calls. Acknowledgements are warm duplicate acknowledgements; renewal returns/replaces its envelope each time. Final durable states are asserted. Rust allocations are counted across the caller/worker threads; the SQLite C allocator is outside that counter. These are allocation probes with no timing claims, no first-ack throughput claim and no global overhead percentage.
