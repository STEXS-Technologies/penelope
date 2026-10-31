# Concurrent admission and commit qualification

Verify with `python3 docs/verification-data/2026-10-31-inbox-concurrency/verify.py`. It checks source/archive hashes, two intentional gated-race failures, all eight cases in 20 repeated processes, regenerated summary/model counts and final qualification logs. This verifies recorded evidence rather than independently authenticating logs or exhausting all schedules.

Reproduce in a disposable candidate checkout with stable Rust and 1.85.0. Create target/inbox-concurrency and target/qualification-temp. Copy controls.py, repeat.py and summarize.py into the former, clear RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER and set TMPDIR to the absolute project qualification-temp directory. Run:

```sh
python3 target/inbox-concurrency/controls.py
python3 target/inbox-concurrency/repeat.py
python3 target/inbox-concurrency/summarize.py
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
```

controls.py temporarily disables the consumed-input guard or suppresses source insertion for inp_competing while retaining the seed's binding. It requires the respective seeded race to report two successes instead of one and fail, restores production source in finally, and reruns the full process suite. Do not run concurrent builds/tests during source replacement. Failed control logs are intentional; restored/final qualification must pass.

Each case constructs both public-port serial reference states, imposes explicit receipt/Conflict expectations, and gates two independent workers behind an external BEGIN IMMEDIATE. Each future must enqueue and become Pending before channels release the gate. Public results and all ten process/lease-counter tables must match a complete legal serial history. Reopening must preserve duplicate successes and Conflict rejections without changing rows. No sleeps or assumed thread overlap are used, and simultaneous entry into a particular busy handler is not asserted.

The eight cases include two same-definition seeded next-frontier checks that directly isolate consumed-input/source-event guards. Cross-definition cases also exercise the schema's unique process_heads.process_key constraint. The first matching serial-model index is not an execution-order observation; both models are indistinguishable in the seeded rejection cases. All serial orders are checked explicitly, independently of which order the concurrent run happens to match.

Production sources/schema/manifests are unchanged. This is reliability qualification, not a runtime fix, latency measurement, all-interleavings proof, corruption reconstruction or power-loss test. Plans from Downloads are not included.
