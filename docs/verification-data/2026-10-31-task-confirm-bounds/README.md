# Termination-confirmation generation bounds evidence

The candidate fix and baseline full commits are recorded in `manifest.json` and [the report](../../verification-task-confirm-bounds-2026-10-31.md).

Verify with Python's standard library:

```sh
python3 docs/verification-data/2026-10-31-task-confirm-bounds/verify.py
```

The verifier hashes archived files, validates all 60 repeat-process logs and fixture shapes, checks the baseline counterexample and final qualification summaries, and verifies the archived prior CI status. It does not independently authenticate recorded logs or replace executing tests. Gzip streams use mtime zero. No performance timing campaign was run for this guard.

To reproduce, use a disposable checkout of the candidate commit. Create `target/task-confirm-bounds` and `target/qualification-temp`; copy root Python scripts here into `target/task-confirm-bounds`. Set `RUSTC_WRAPPER=` and `RUSTC_WORKSPACE_WRAPPER=`, and set `TMPDIR` to the absolute qualification-temp path. Run:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
python3 target/task-confirm-bounds/repeat.py
python3 target/task-confirm-bounds/baseline-control.py
```

`repeat.py` runs the new bounds/retirement suite, existing authority identity-column suite and first-append quota suite ten times each on stable and Rust 1.85. Each bounds run completes 12 invalid fixtures (generation 4, u32 maximum or zero; state 1 or 2; NULL or reserved identity) and two full valid retirement/reopen workflows through the last allowed attempt. The positive workflows use public APIs and legitimate recovery transitions. Only the generation-zero corruption fixture disables CHECK constraints; positive excessive generations are legal SQLite rows but violate the task definition's policy.

The baseline-control script temporarily replaces authority.rs with pinned baseline `eaa920ad8a2ecd458a18087cdecf1adad07cb1f6`, requires the expected `Ok(())` vs `Conflict` failure, restores fixed source in `finally`, then reruns complete SQLite unit/process/task suites on both compilers. Run this mutation separately without concurrent builds. The old control's invalid matrix stops at its first failing generation-4/state-1/NULL case; it also passes the valid-workflow test. Its failure is intentional. Final and repeat logs must pass.

Source snapshots distinguish the baseline and fixed authority and include the final integration-test module. Initial `regression.log.gz` predates only case-order/fixture CHECK-toggle cleanup; final repeated and full-suite logs describe the final source. The test snapshots and final counterexample use excessive positive generation first, requiring no disabled constraints for the observed baseline failure.

Current local qualification excludes the separate fuzz crate and optional StateChronicle gate. `previous-ci.json` records their success for the prior baseline only. It is not final-code CI. Normal adapter APIs already prevent out-of-policy start/fence rows; this fix rejects invalid confirmation requests even when matching externally modified metadata exists. No release plan from Downloads is included.
