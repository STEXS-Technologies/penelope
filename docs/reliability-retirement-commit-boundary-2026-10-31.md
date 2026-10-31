# Retirement rollback at COMMIT — 2026-10-31

New tests cover a deferred SQLite failure after retirement has deleted a completed task's history, blobs, and head inside its transaction. Existing behavior rolls those deletions back and preserves the stopped execution identity. The change is test-only and adds no release overhead.

Baseline: `cf16f77bee0e867f19fbdaf8db110ea450ba967c`.

## Observation and recovery

The fixture always contains a completed target task and an unrelated task. A temporary `AFTER DELETE` trigger on `task_heads` inserts an orphan into a table with a deferred foreign key. A test-only SQL function inspects the target's records, blobs, and head from inside the transaction; all three counts must be zero when it runs. The deferred constraint then rejects `COMMIT` with the existing `TaskPortError::Conflict` mapping.

After failure the connection must be in autocommit mode, the maintenance cache must be invalidated, and all five task tables must exactly match their pre-call snapshot. The temporary orphan row must also be rolled back. Both original records and output bytes must remain readable.

The test inserts the missing foreign-key parent while leaving the same trigger installed, then retries retirement on the same journal. It requires another successful final-delete observation, the exact retirement receipt, and a durable tombstone with the expected frontier and canonical final-record digest. Only the target's head, records, and blobs disappear. The unrelated task's complete snapshot and the stopped target's execution identity/state remain intact.

After reopening, stale reads, commits, retirement attempts, and start reservations must be rejected. A wrong stopped identity must fail, while repeating confirmation of the retained identity succeeds without changing the logical snapshot. A start reservation is required to fail closed; its error classification is not tightened by this test.

## Matrix and controls

The 12 cases combine:

- Warm target cache, cold journal, or warm other-scope cache.
- A stopped generation with no execution identity or with a retained execution digest.
- Output bytes shared with the input blob or stored as a distinct blob.

Stable Rust and Rust 1.85 each passed 20 repetitions: **240 cases per toolchain, 480 combined**, excluding separate qualification runs.

Three temporary mutations were rejected and restored:

1. Swallowing the retirement COMMIT error returns a false successful receipt.
2. Disabling rollback on transaction drop leaves the connection in the failed transaction.
3. Broadening the deletion predicate to all scopes causes the deletion probe to observe more than one retired head; the test also checks that the unrelated task survives the successful retry.

This covers completed histories with a real deferred-constraint error. It does not simulate every terminal state, disk failure, process crash, or possible SQLite COMMIT error, and makes no performance claim.

## Evidence

[Verification archive](verification-data/2026-10-31-retirement-commit-boundary/) includes source hashes, toolchain versions, repeated results, negative controls and their driver, and qualification logs. Its read-only verifier confirms the exact checkout, all 40 recorded repetitions, three controls, and unchanged release sources:

```sh
python3 docs/verification-data/2026-10-31-retirement-commit-boundary/verify.py
```

Qualification covers stable workspace tests, Rust 1.85 SQLite tests, strict workspace Clippy across all targets/features, formatting, dependency boundaries, and AST graph refresh. The only Rust change appends retirement cases to the existing test-only task commit boundary module.
