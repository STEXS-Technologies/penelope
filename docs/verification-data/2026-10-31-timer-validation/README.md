# Timer scheduling validation qualification

Verify with `python3 docs/verification-data/2026-10-31-timer-validation/verify.py`. The verifier checks archive/source hashes, all 1,280 allocation samples, the intentionally failing checksum-bypass control, 20 repeated regression processes and final qualification logs. It validates recorded evidence rather than independently authenticating logs or measuring speed.

For reproduction use a disposable candidate checkout with stable Rust and 1.85.0. Create target/timer-validation and target/qualification-temp. Copy compare.py, control.py, repeat.py and summarize.py into the former and decompress baseline-leases.rs.gz there. Clear RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER and set TMPDIR to the absolute project qualification-temp directory. Run:

```sh
python3 target/timer-validation/compare.py
python3 target/timer-validation/control.py
python3 target/timer-validation/summarize.py
python3 target/timer-validation/repeat.py
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
```

compare.py temporarily installs the pinned baseline leases.rs, runs the process suite and builds/copies the release probe, then repeats for the candidate, always restoring the candidate in finally. control.py intentionally bypasses stored_timer checksum validation, requires the first checksum fixture to fail, restores production source and reruns the test. Do not run concurrent builds/tests during either source replacement. The failed control log is expected; restored and qualification logs must pass.

The probe samples warm duplicate scheduling, repeated cancellation, duplicate acknowledgements and successive renewals; it does not separately sample first insertion/cancellation/acknowledgement. All fixtures use public ports and verify durable final state. Rust System allocation requests across caller and worker threads are counted; SQLite C allocations and peak memory are outside the counter. No wall-clock or global overhead claim follows. Results can differ with toolchains/dependencies.

Each regression process covers two durable row types and six fault types after warming both SELECT statements, checks rejected writes against exact four-table snapshots and repeats rejection after reopening. The baseline passes this same preservation test. Twenty repeated processes cover 240 fault fixtures and 720 rejected schedule/cancel requests. The mutation control stops at the first intentional failure.
