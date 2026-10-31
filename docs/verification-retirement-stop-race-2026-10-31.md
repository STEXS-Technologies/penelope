# Retirement versus stop confirmation, 2026-10-31

Added a regression test for concurrent task retirement and stop confirmation on separate SQLite connections. Production code is unchanged. This extends the reservation/fence race coverage to the boundary where history can be deleted only after execution termination is durably confirmed.

The fixture commits Intent, Started and Completed records, then fences the generation. Two identity cases cover a reserved execution digest and an undispatched generation with no execution identity. Preparation explicitly verifies that retirement before stop confirmation returns Conflict and leaves all five tables unchanged. A separate serial control confirms the matching identity, then verifies successful retirement.

For each identity case, eight races start retirement and stop confirmation behind a three-party barrier. Confirmation must succeed. Retirement must either succeed after confirmation or return Conflict before it; after Conflict, the first four tables must remain exactly unchanged and a retry must succeed. Unexpected errors fail the test. The test verifies the exact retirement frontier and final record digest, zero retained record/head/blob rows, one retirement row, and exactly one stopped authority row with its original execution identity.

After reopening, a wrong stop identity must return Conflict without changing rows, matching confirmation remains idempotent, delayed intent commits and reads return Conflict, repeat retirement returns Conflict, and reservation is rejected. A final independent connection again rejects reads. Snapshots compare all five tables after rejected and idempotent operations. These are SQLite transaction/reopen checks, not simulated power-loss tests.

Twenty repeated runs per toolchain exercised 640 race histories and 1,280 contending requests in total. Stable Rust observed 320 retire-first histories. Rust 1.85.0 observed 319 retire-first and one stop-first history. Scheduling is not guaranteed; both serial orders are also exercised explicitly by the preparation rejection and confirmation-before-retirement control. Each run includes two such serial controls in addition to the 16 races.

A controlled mutation disabled the active-generation retirement guard (`if active && false`). The new test failed during preparation: retirement incorrectly succeeded before stop confirmation. This validates the prerequisite assertion; it does not establish that every possible concurrency bug is detected. The production file was restored byte-for-byte to HEAD, and all 45 task integration tests then passed again.

Validation: all 45 task tests on stable Rust and Rust 1.85.0, 20 repeated new-test runs on each, strict workspace Clippy, formatting and layer checks. No runtime performance change is claimed.

[Raw logs, mutation metadata and verifier](verification-data/2026-10-31-retirement-stop-race/) are committed. Run `python3 verify.py` in that directory to check the archived evidence. Reproduce the races with `RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= TMPDIR="$PWD/target/qualification-temp" cargo test -p penelope-sqlite --test tasks --all-features retirement_racing -- --nocapture`; repeat 20 times, then repeat with `cargo +1.85.0` and a separate `CARGO_TARGET_DIR`. Source hashes identify the test and unchanged production implementations.
