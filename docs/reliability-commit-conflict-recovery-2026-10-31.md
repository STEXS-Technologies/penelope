# Recovering a losing process commit

This audit extends the competing-batch fixture through conflict recovery. It
found no production defect. Production code remains identical to
`f4b0094c9f14903078b24202fe4ee8232201d750`; the new coverage adds no runtime cost.

Each fixture starts with either an empty process or a publicly committed seed.
Two independent connections submit different valid batches at one frontier,
each with an input, two outcomes, two actions and a canonical source event.
The existing gated race and both explicit serial references require exactly
one original receipt and one `Conflict`, with an exact ten-table state match.

The added recovery sequence then:

1. Reopens the store and confirms that retries of the original winner and loser
   preserve state and return duplicate success and `Conflict`, respectively.
2. Reloads the complete public outcome page, derives the new frontier, and
   constructs a valid fixture batch there using the loser's unconsumed input.
3. Commits that batch with an original receipt and verifies exact decoded inputs,
   outcomes, actions, source bindings, receipt count and head frontier against
   seed + winner + recovered batch.
4. Closes and reopens the recovery store, then verifies unchanged duplicate
   retries of all committed batches and continued rejection of the stale loser.
5. Attempts a fresh batch at the next frontier with the recovered input again.
   Fresh outcome/action/step identities and no canonical source event remove
   alternate collision guards. It must return `Conflict` and preserve all rows.
6. Replays the complete log and checks the exact expected history and no
   continuation.

This is a storage-port fixture. Applications still construct a new business
decision from recovered state; this change adds no automatic retry policy.

All 66 process integration tests pass on stable Rust and Rust 1.85.0. Twenty
additional executions per toolchain each pass 16 gated races and recovery
sequences: 640 recovery cycles, 1,280 competing requests and 1,280 serial reference
histories. The old race and admission tests also pass after their shared record
verification helper was extracted. Strict workspace Clippy, formatting, layer
checks and an AST-only Graphify update pass.

A temporary negative control removed the consumed-input rejection in atomic
commit. The new test failed when the second consumption returned a successful
receipt instead of `Conflict`. It stops at the first empty-process recovery;
separate mutation coverage for every fixture is not claimed. The production
source was restored before qualification.

[Logs, source hashes and verifier](verification-data/2026-10-31-commit-conflict-recovery/)
record the qualification. Run from the repository root:

```sh
python3 docs/verification-data/2026-10-31-commit-conflict-recovery/verify.py
```

Coverage is a bounded two-connection workflow, without a claim about arbitrary
application decisions, scheduling fairness or power-loss behavior. Release plans
remain outside the repository.
