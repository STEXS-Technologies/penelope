# Competing process batches at one frontier

This audit adds coverage for two different, valid atomic batches competing to
advance the same process scope at the same expected sequence. Production code
is unchanged; this is additional reliability evidence, with no runtime overhead
or new speed claim.

Earlier tests covered identical commits, competing definition scopes, and
consumed inputs or reused source events. The new test covers an empty process
and a process with one publicly committed seed. Each competitor contains a new
input, two outcomes, two actions and a distinct canonical source event. All
input, outcome, action, step and source identifiers and payload digests differ,
while the pinned definition scope and expected frontier match.

Two serial reference databases run both operation orders for every race.
Independent expectations require the first batch to return an original receipt
and the other to return `Conflict`. Two separate store connections then race on
a third database. An external `BEGIN IMMEDIATE` lock holds writes while both
public futures are polled and confirmed pending through channels; releasing the
lock lets them compete. This confirms both requests are in flight, without
assuming simultaneous entry into SQLite's busy handler or relying on sleeps.

The complete result pair and exact rows in all ten process/lease tables must
match one serial reference. Additional checks decode the persisted inputs,
outcomes and actions and compare them directly with the seed plus winning
request. The source-event bindings, receipt count and head frontier must also
match those requests. No losing input, outcome, action or source binding may
remain. After reopening, the winner retries as a duplicate and the loser still
returns `Conflict`; neither retry may change any snapshotted row.

Qualification on stable Rust and Rust 1.85.0:

- All 65 process integration tests passed on both toolchains, including the
  existing admission, source-consumption and duplicate-commit cases.
- Twenty additional executions per toolchain passed, each containing eight
  empty-frontier and eight next-frontier races: 640 gated races, 1,280 competing
  requests, 1,280 serial reference histories and 1,280 reopened retries.
- Workspace strict Clippy, formatting and layer checks passed. Graphify was
  refreshed with AST extraction.

The matching serial models were distributed as follows. Index 0 is left-first
and index 1 right-first. These are result/state matches, not an instrumented
trace of the SQLite execution order; neither distribution is required to pass.

| Toolchain | Fixture | Model 0 | Model 1 |
| --- | --- | ---: | ---: |
| stable | Empty frontier | 107 | 53 |
| stable | Next frontier | 116 | 44 |
| Rust 1.85.0 | Empty frontier | 100 | 60 |
| Rust 1.85.0 | Next frontier | 116 | 44 |

A temporary negative control removed both the digest comparison and persisted
batch verification from the existing-commit path, effectively treating any
batch at an occupied frontier as an exact retry. The new test caught two success
receipts for different batches. It stopped at the first empty-frontier failure;
this control does not establish separate mutation coverage for every fixture.
Production source was restored before qualification and remains identical to
`cddd2165173091cd9e5a3bbd4fa91a6cb0cc8259`.

[Evidence and verifier](verification-data/2026-10-31-competing-process-commits/)
include compressed logs, repetition counts and source hashes. From the repository
root, run:

```sh
python3 docs/verification-data/2026-10-31-competing-process-commits/verify.py
```

This covers two competing connections and two-outcome/two-action batches. It does
not prove arbitrary-load fairness, every corruption scenario or power-loss
behavior. No release-plan files are included.
