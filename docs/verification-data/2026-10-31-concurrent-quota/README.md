# Shared quota admission across independent SQLite workers

Candidate and baseline full commits are in `manifest.json` and [the report](../../verification-concurrent-quota-2026-10-31.md). Verify with Python's standard library:

```sh
python3 docs/verification-data/2026-10-31-concurrent-quota/verify.py
```

This checks source/archive hashes, two intentional missing-guard failures, restored/full qualification logs and all nine race cases in each of 20 repeated processes. It regenerates `summary.json`, including every observed winner. It does not independently authenticate logs, exhaust all schedules, derive retained-byte accounting or measure latency. Gzip streams use mtime zero.

Reproduce in a disposable candidate checkout. Create `target/concurrent-quota` and `target/qualification-temp`, copy Python scripts into the former, clear RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER and set TMPDIR to the absolute qualification-temp directory. Run:

```sh
python3 target/concurrent-quota/controls.py
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
python3 target/concurrent-quota/repeat.py
python3 target/concurrent-quota/summarize.py
```

The controls script requires process.rs to match the pinned baseline, disables only the shared byte guard or only the process_usage count guard, and expects the corresponding first payload race to admit two writes and fail. These controls stop at their first failing case; they do not execute the entire faulty matrix. It always restores production source in `finally` and reruns the complete process suite. Do not run concurrent builds/tests while the source is temporarily replaced. Failed control logs are intentional; all restored and final logs must pass.

Each normal scenario measures both operations independently with generous limits to construct two legal serial reference states and determine their charged bytes. The test then chooses a byte budget that fits either individual write but not both, or a one-process admission limit for different process identities. A separate connection holds BEGIN IMMEDIATE while both public futures enqueue and report Pending on independent workers. Channels confirm admission of both requests before releasing the lock; no sleeps or assumed overlap are used.

The test requires one success, one QuotaExceeded and exact equality of all ten process/lease-counter tables with the winning serial state. It checks that each reference state rejects its competing operation atomically under the same limit, then reopens the raced database and checks an unchanged duplicate retry and another unchanged quota rejection. Timer scenarios begin from a publicly committed timer action. Payload byte-budget coverage includes different definition digests sharing one tenant/process counter.

The changes add integration tests and evidence; production Rust sources and Cargo manifests/lockfile remain identical to the baseline. Runtime quota accounting, schemas and earlier performance results are not changed. This evidence does not reconstruct or repair usage metadata deleted/rewritten by an external connection, account physical database bytes or independently prove crash recovery.

`previous-ci.json` records successful baseline CI, including bounded fuzzing and optional StateChronicle conformance. Local qualification does not claim fresh runs of those separate gates. Release plans from Downloads are not included.
