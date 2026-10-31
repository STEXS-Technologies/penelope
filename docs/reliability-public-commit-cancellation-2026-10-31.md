# Public commit cancellation and lost acknowledgements, 2026-10-31

A new SQLite integration test proves the public commit/replay/retry boundary when the caller drops its commit future without observing the result. Successful admitted commits remain durable; a late SQL error rolls back all transactional writes. After reopening, retries distinguish an already committed decision from a decision whose atomic input admission rolled back. This adds test coverage only: production code, dependencies, schema and execution overhead are unchanged.

## Scenarios and synchronization

The complete matrix has 16 cases:

| Dimension | Cases |
| --- | --- |
| Input admission | Separate durable preacceptance; input admitted atomically with the decision |
| Caller cancellation | Before commit can acquire its write transaction; after the worker publishes the result but before the caller reads it |
| Existing history | Empty frontier; seeded two-outcome/two-action decision |
| Transaction outcome | Success; SQL abort on the target decision's second action insert |

Each target decision contains an input, two outcomes, two external-effect actions and a canonical source binding. An independent connection holds `BEGIN IMMEDIATE` while the test polls the public commit and a following public replay to `Pending`. Both requests are admitted to the same worker. The test checks that the locked database has not changed. For cancellation after completion, a live future's registered waker signals publication of its result; that result is never polled again. For cancellation before commit, the future is dropped before the external write lock is released.

A retained replay future is queued behind the commit. Its completion establishes that the worker has drained the admitted commit, including after caller cancellation. The test does not depend on canceled futures retaining or waking their registrations. Channels and the external database lock enforce the order; sleeps do not establish the tested interleavings. Five-second channel deadlines detect stalled progress.

## Durable assertions

Each case compares every row in all ten process and lease-counter tables with an independently executed serial reference, both immediately after the canceled operation and after recovery. The following replay must return the exact expected complete outcome history and continuation state.

The injected SQL abort occurs after both target outcomes, its input/source binding and its first action have been inserted. Rollback must restore the exact pre-commit snapshot. Separately preaccepted inputs remain durable; atomically admitted inputs do not survive that failed decision. Clearing the trigger and retrying after a clean reconnect must produce the same final durable state as the healthy serial decision.

When the unobserved operation succeeded, the reopened retry must report a duplicate input. When it rolled back, the retry must report a duplicate input only if a separate durable preacceptance existed. Receipts validate against the original commit. Direct typed-row checks verify the complete expected inputs, outcomes, actions, source bindings, commit count and head. A further identical retry must be a duplicate without mutations; a valid commit with conflicting payload contents at the occupied frontier must fail with `Conflict` and leave all rows unchanged. Final public replay must match all expected decisions exactly.

## Qualification and sensitivity

The final source passed 20 complete matrices on each of stable Rust 1.99.0 and Rust 1.85.0: 640 canceled commits with queued replay barriers, including 320 forced SQL rollbacks. The complete SQLite suites passed on both toolchains: 36 unit, 67 process integration and 45 task integration tests. Strict workspace all-target/all-feature Clippy excluding the fuzz package, formatting and layer checks passed. Graphify was updated.

A temporary negative control changed the existing-commit receipt to falsely report `duplicate_input=false`. The new lost-ACK retry assertion rejected it with `false` versus the required `true`. The script restores production source in `finally`; archived source hashes verify that the runtime matches parent `e488365a84b3462a4b2ffa0ca5d7eb25de94b57e` exactly for the covered modules. This control demonstrates sensitivity to incorrect retry evidence; it does not simulate a process crash or power loss.

This extends the earlier generic worker cancellation and panic tests through the real public atomic commit, serial queue, durable process tables and reopen/retry APIs. It measures neither performance nor power-loss durability and introduces no runtime fix. Reopening here is a clean connection replacement after the queued replay confirms completion.

## Reproduction

Run `cargo test -p penelope-sqlite --test process canceled_public_commits --all-features -- --nocapture` for one matrix. Use the archived `stress.py stable <fresh-directory>` and `stress.py msrv <fresh-directory>` for 20 repetitions per toolchain from the repository root. The latter requires Rust 1.85.0.

Run `python3 docs/verification-data/2026-10-31-public-commit-cancellation/verify.py` to validate archived hashes, unchanged production sources, all 40 complete repetition matrices, qualification and the negative control without mutating source. Source checks require this version of the checkout. The optional archived `negative-control.py <fresh-directory>` temporarily alters production source and restores it; run it only in an idle checkout with no concurrent build, test or source operations.

[Evidence, scripts and verifier](verification-data/2026-10-31-public-commit-cancellation/).
