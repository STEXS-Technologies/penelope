# Task journal cache at failed COMMIT — 2026-10-31

New deterministic tests protect the boundary between SQLite durability and the task journal's cached projection. A transaction that fails at `COMMIT` must leave the prior history, blobs, head, and cache intact. Existing release behavior passes; this change adds tests and shares their snapshot helper.

Baseline: `4e02668e729ca79609f42463bb4b276ca89cab26`.

## Fault and case matrix

A temporary child table has a deferred foreign key to an empty temporary parent. An `AFTER INSERT` trigger on the real `task_records` table inserts an orphan child and calls a test-only SQLite function. The function count proves that the final record insert ran. Foreign-key enforcement must be enabled on the actual journal connection. The public `TaskJournal::commit` must then return `TaskPortError::Conflict` when the deferred constraint rejects `COMMIT`.

| History position | Cache modes | Target records | Cases per repetition |
| --- | --- | --- | ---: |
| Later append | Warm target, cold reopen, warm other scope | Started without a blob, checkpoint with new bytes, checkpoint deduplicating the input blob | 9 |
| First intent | Cold, warm other scope | Intent with a new input blob and head | 2 |

Every failure must preserve an exact snapshot of all five task tables and the complete pre-call cached tuple, leave no orphan child row, and restore autocommit mode. This checks the retained cache identity, frontier, digest, usage, and data-version token together, including an unrelated scope's cached projection.

The positive control repairs the foreign key by inserting its missing parent **without removing the trigger**. Retrying on the same journal must then succeed and increment the same final-insert counter again. The published cached projection must equal the independently advanced projection. Stored records and supplied blobs must be readable, stale retries must return Conflict without writes, and reopening must preserve the first and target records and reject another stale retry without changing storage.

Stable Rust and Rust 1.85 each passed 20 repetitions: **220 cases per toolchain, 440 combined**. These counts cover 360 later append cases and 80 first-intent cases and exclude separate qualification runs.

## Negative controls

Three temporary mutations fail both new tests and are restored afterward:

- Ignoring the task transaction's commit error produces a false successful receipt.
- Publishing the candidate cache before committing changes the cached frontier despite the required error.
- Setting transaction drop behavior to ignore rollback leaves the connection inside a failed transaction.

The tests detect each specific failure through its corresponding assertion. No production failpoint or timing hook is added.

## Scope and verification

The fault covers real deferred-constraint failure at `COMMIT`; it does not simulate every database or operating-system failure. There is no throughput or latency claim. Release implementation changes are limited to `cfg(test)` module declarations. The previously added process-boundary tests import the same test-only snapshot helper rather than loading a duplicate module.

[Evidence archive](verification-data/2026-10-31-task-commit-cache-boundary/) contains repetition logs, case counts, negative controls and their driver, source hashes, toolchain versions, and final qualification. Verify the exact checkout without rerunning tests:

```sh
python3 docs/verification-data/2026-10-31-task-commit-cache-boundary/verify.py
```

The verifier checks the archive, source hashes, the exact test-only differences from the pinned parent, all 40 repetition results, all three negative controls, and final qualification records. Qualification covers stable workspace tests, Rust 1.85 SQLite tests, strict workspace Clippy across all targets/features, formatting, dependency boundaries, and an AST graph refresh.
