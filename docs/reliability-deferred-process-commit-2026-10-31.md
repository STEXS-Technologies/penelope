# Deferred SQLite process COMMIT failures — 2026-10-31

The process adapter now has deterministic coverage for a database error at `COMMIT`, after successful SQL writes. This extends statement-abort, cancellation, and panic coverage with a distinct transaction boundary. Existing behavior passes; no release implementation or performance behavior changes.

Baseline: `f2c0696eec5894a53350edd7442c154533e5c73d`.

## Fault and observation

A test-only child table references an empty parent with `DEFERRABLE INITIALLY DEFERRED`. The worker connection is opened through the real schema setup and must have foreign keys enabled. An `AFTER INSERT` trigger on `process_commits`, or on `process_inbox` for standalone admission, inserts an orphan child. The statement succeeds, while transaction commit fails.

A worker-local SQLite function records which trigger stages ran. Atomic commit cases require the receipt-insert stage and the final inbox-consumption update stage, in order. Outcome-only append cases require the receipt-insert stage. Inbox admission requires its final insert stage. These observations distinguish this fault from an earlier statement abort. The public operations must return `PortError::Conflict` under the adapter's existing constraint-error mapping.

The cases use the public async `ProcessStore::commit`, `append_outcomes`, and `Inbox::accept` operations. Only fixture construction and observation use private access, to register the SQLite function before handing the connection to the normal database worker. There are no sleeps used to choose an interleaving; the bounded waiter parks only while awaiting the real operation.

## Case matrix and recovery requirements

| Operation | Dimensions | Cases per repetition |
| --- | --- | ---: |
| Atomic commit | Short/128-character scope identifiers × empty/seeded history × fresh/preaccepted input × 1/128 outcomes and actions | 16 |
| Outcome-only append | Short/128-character scope identifiers × empty/seeded history × 1/128 outcomes | 8 |
| Inbox admission | Short/128-character scope identifiers × empty/seeded history | 4 |

Every case requires:

- The expected final-write stage observations and a typed failure, never a successful receipt.
- Exact before/after equality across all ten process tables and both fault-probe tables after the error.
- A subsequent job on the same database worker must observe autocommit restored and no orphan child rows.
- Removing the fault trigger and retrying on the same adapter must succeed. Atomic commit receipts must preserve whether the input was preaccepted; standalone inbox retry must be fresh.
- Reopening and retrying must return the expected duplicate behavior without changing the durable snapshot. Outcome-only append also verifies exact replayed outcomes and the end cursor.

Stable Rust and Rust 1.85 each passed 20 repetitions: **560 cases per toolchain, 1,120 combined**. That includes 640 atomic commit cases, 320 append cases, and 160 inbox admission cases. These counts exclude separate qualification runs.

## Negative controls

Three temporary implementation mutations were tested and restored:

1. Ignore the process transaction's commit error and return a receipt: the atomic commit and outcome-only append cases fail because success replaces the required typed error.
2. Ignore the standalone inbox transaction's commit error: admission cases fail for the same false acknowledgment.
3. Set the process transaction's drop behavior to ignore rollback: atomic commit and append cases fail because the worker connection remains inside the failed transaction.

These controls demonstrate that the tests detect false acknowledgments and a dirty connection after a failed commit. The unchanged implementation restores the connection before serving the next request.

## Scope and evidence

This injects a deferred constraint error through real SQLite transactions. It does not simulate disk failure, an operating-system crash, WAL damage, or every possible COMMIT error. It makes no throughput or latency claim. The only change to the existing implementation file is a `cfg(test)` module declaration.

[Archived evidence](verification-data/2026-10-31-deferred-process-commit/) includes repetition logs and counts, negative-control logs and driver, source hashes, toolchain versions, and final qualification logs. Run its read-only verifier from this exact checkout:

```sh
python3 docs/verification-data/2026-10-31-deferred-process-commit/verify.py
```

The verifier checks source and archive hashes, confirms that release source differs from the parent only by the test module declaration, and checks all 40 recorded repetition results and the three negative controls. It verifies the recorded evidence without rerunning the tests.

Final qualification includes stable workspace tests, Rust 1.85 SQLite tests, strict workspace Clippy across all targets/features, formatting, dependency boundaries, and an AST graph refresh. Exact commands and statuses are recorded in `qualification.json`.
