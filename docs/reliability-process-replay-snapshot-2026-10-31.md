# Process replay during a concurrent append

This audit adds a deterministic regression test for the transaction snapshot
shared by replay's head/tail lookup and its separate outcome query. It found
no production defect. Runtime query code, schema, dependencies and durability
settings are unchanged; `process.rs` gains only a `cfg(test)` module declaration.

The test warms the public replay queries, then installs a reader-local TEMP view
and scalar callback through the existing private database worker. Reading the
main head table pins a real database snapshot. The callback pauses the reader
and signals another connection to append two outcomes through the public atomic
commit port. Only after that commit returns a validated original receipt does
the writer release the reader. The SQL function and TEMP view exist solely in
the test connection; no production hook is added.

Four profiles cover:

| Profile | Initial outcomes | Cursor | Limit | Required pinned result |
| --- | ---: | ---: | ---: | --- |
| Empty process | 0 | 0 | 4 | Empty page without continuation |
| Whole history | 2 | 0 | 4 | Original two outcomes without continuation |
| Limited page | 2 | 1 | 1 | Original last outcome without continuation |
| Old end | 2 | 2 | 4 | Empty page without continuation |

For the empty profile, the TEMP view scans the empty main table before returning
a synthetic matching scope with NULL head values. This preserves the missing-head
result while allowing the callback to pause after an empty snapshot is pinned.
For populated profiles, adding the callback's zero result to the existing head
value preserves its original value.

Each pinned page must exactly match the original typed history. A subsequent
public read must see the newly committed history; the limited page now has a
continuation. The earlier owned page must remain unchanged. After reopening,
the full history must match all committed outcomes, and an exact retry of the
append must return a validated duplicate receipt.

A negative control replaced the read transaction with a bare connection
reference, allowing the head lookup and outcome query to use separate snapshots.
The test failed with `Invariant` after a valid append. It stopped at the first
empty-process failure; mutation coverage of every profile is not claimed. The
original transaction was restored before qualification.

Twenty additional executions per toolchain passed on stable Rust and Rust
1.85.0: 160 forced append-during-read interleavings across all four profiles.
SQLite unit and integration suites passed on both toolchains: 36 unit,
66 process and 45 task tests per toolchain. Strict workspace Clippy, formatting and layer checks passed, and
Graphify received an AST-only update. These are functional checks, without a
new latency or overall-overhead claim.

[Evidence and verifier](verification-data/2026-10-31-process-replay-snapshot/)
include compressed logs, the mutation patch and source hashes. From the
repository root, run:

```sh
python3 docs/verification-data/2026-10-31-process-replay-snapshot/verify.py
```

Coverage is one reader and one writer in WAL mode, without a claim about every
query interleaving, arbitrary-load fairness or power-loss behavior. Release
plans remain outside the repository.
