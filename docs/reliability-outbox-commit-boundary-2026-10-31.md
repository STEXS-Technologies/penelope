# Outbox deferred COMMIT failure audit, 2026-10-31

Outbox claim, acknowledgement and renewal now have explicit regression coverage for a failure at SQLite `COMMIT`, after their update statements have succeeded. Production code already propagates these errors and rolls back correctly; this change adds tests without changing runtime behavior or performance.

The new test covers twelve combinations: short/maximum-length process scope identifiers, one/four actions, and all three operations. It installs a deferred foreign-key violation through an `AFTER UPDATE` trigger. A worker-connection SQL callback records every completed write, including all four writes in the larger claim, before `COMMIT` fails with `PortError::Conflict`. This distinguishes a failed commit from an early statement abort.

For every case the public asynchronous operation returns the error, the worker connection returns to autocommit, and an exact logical snapshot of all ten process tables plus both probe tables matches the original state. In particular, a failed multi-action claim cannot consume lease tokens or partially advance attempts. A failed acknowledgement cannot publish completion, and failed renewal cannot replace the stored envelope or extend expiry.

The test repairs the missing foreign-key parent while preserving the trigger, then retries on the same worker. The callback confirms the same writes execute again and the commit succeeds. It independently checks the next token, every returned token, delivery attempt, exact stored lease bytes, expiry and acknowledgement flag. Other process tables remain unchanged. A new adapter connection sees no eligible unexpired actions, preserves the successful snapshot, and rejects the original envelope after successful renewal. After removing the test-only trigger, every current envelope can be acknowledged through the reopened adapter, including an already acknowledged envelope's idempotent retry.

The acknowledgement and renewal cases with four actions also verify that the three untouched action envelopes, attempts, expiries and acknowledgement flags remain correct. The claim cases test fresh claims; expired-lease reclaim races and early statement failures retain their separate existing coverage. Deferred constraints exercise SQLite's commit error/rollback path; these cases do not simulate disk-full, I/O failure or process death.

## Qualification

Twenty repetitions on stable Rust and twenty on Rust 1.85 execute 480 explicit fault/retry/reopen cases. Four source-mutating negative controls prove the test rejects ignored commit errors in each of claim, acknowledgement and renewal, and detects a failed transaction left open by disabling drop rollback. The mutation driver restores production source in a `finally` block; no other build, test or graph jobs run during these controls.

Final all-feature stable workspace tests excluding the fuzz package, all-feature Rust 1.85 SQLite tests, workspace all-target/all-feature strict Clippy, formatting and layer checks pass. The SQLite suite has 43 unit tests, 72 process integration tests and 45 task integration tests. Graph output is refreshed locally and remains outside the commit.

[Archived logs, hashes, drivers and verifier](verification-data/2026-10-31-outbox-commit-boundary/) pin the test and its helpers, preserve all repetitions and four failing negative controls, and verify release sources against parent `c909967d1413df2858afec7f4ea5af8eacc7d9b4`. Run `python3 docs/verification-data/2026-10-31-outbox-commit-boundary/verify.py` at this version for read-only evidence verification. No application slowdown claim follows from these reliability tests.
