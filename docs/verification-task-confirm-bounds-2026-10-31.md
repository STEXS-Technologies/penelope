# SQLite termination-confirmation generation bounds — 2026-10-31

Fix: `af26363e74206c31d06d5c922dbd6873f520e16b`. Baseline: `eaa920ad8a2ecd458a18087cdecf1adad07cb1f6`.

`confirm_stopped` now rejects generation zero or a generation greater than the pinned definition's `recovery.max_attempts` with `Conflict` before issuing SQL. This matches `begin_fence`'s existing policy check. Previously a matching runtime-generation row in state 1 or 2 could make confirmation return success even though fencing that generation was forbidden. A state-1 row could be marked stopped. The definition fingerprint and exact execution-identity checks remain intact.

Normal adapter starts/fences already enforce these bounds. The counterexample requires an out-of-policy request and matching metadata created or changed outside those normal APIs. This is an integrity/input-validation fix, not evidence that normal task execution generates invalid attempts. Positive out-of-policy generations satisfy SQLite's schema constraint; generation zero requires deliberately bypassing CHECK constraints in the corruption fixture.

## Evidence

The new negative matrix covers generations 4 (one beyond the fixture's maximum 3), `u32::MAX`, and zero; states 1 and 2; and both NULL/unreserved and matching reserved execution identities. All 12 cases require rejection by fencing and confirmation, no durable mutation, and confirmation rejection again after reopening.

The positive regression drives legitimate task histories through all three generations, using timeout/recovery/restart transitions. It covers reserved and unreserved executions, repeated valid confirmation, confirmation at the last allowed attempt, terminal completion, retirement, confirmation of retained tombstones, reopening, and wrong-identity rejection without changing rows. It ensures the new check preserves maintenance/idempotency behavior after history has been deleted.

The final regression run against the pinned old authority.rs returns `Ok(())` where `Err(Conflict)` is required on the first generation-4/state-1/NULL case, without disabling schema constraints. The old code's valid-workflow test passes. The control restores fixed source in `finally` and reruns complete SQLite unit, process and task suites on stable and Rust 1.85; both pass. The baseline control stops its invalid matrix at the first failing case and does not independently prove all 12 old results.

Sixty additional isolated stable/MSRV test processes (ten repeats per compiler of three suites) cover:

- 240 invalid-generation fixtures and 720 rejected API calls, with unchanged-row checks;
- 40 valid full retirement/reopen workflows through the last allowed attempt;
- 540 existing identity-metadata fixtures across runtime states;
- 240 first-append quota fixtures, including 80 atomic over-quota rejections.

Final stable workspace all-target/all-feature tests excluding the separate fuzz crate, SQLite all-target tests on Rust 1.85, strict workspace Clippy, formatting, layer boundaries and graphify update passed. SQLite has 27 unit, 51 process and 42 task tests. Complete SQLite suites passed again on both compilers after the counterexample control restored production source.

The prior pushed baseline's GitHub Actions run `41178133091` completed successfully, including bounded fuzzing, optional StateChronicle conformance, stable/MSRV verification, portable-core allocation checks and supply-chain checks. Its JSON status is archived; it does not establish final-code CI. Local qualification above excludes the separate fuzz crate and does not claim a fresh optional StateChronicle run.

The production change adds only the generation-policy guard. It preserves the valid request's single UPDATE. It adds no query or allocation and changes no schema, dependency, or wire format. No timing benchmark was run for this guard, so no speed or overall slowdown percentage is claimed.

[Evidence and reproduction](verification-data/2026-10-31-task-confirm-bounds/README.md) includes the baseline counterexample, source snapshots, final qualification/repeat logs and a standalone verifier. The release plan in Downloads was not copied into the repository.
