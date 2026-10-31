# Outbox acknowledgement versus reclaim

A delayed acknowledgement must not complete an action after another SQLite
connection has reclaimed it under a replacement lease. This audit adds a
cross-connection regression test; it found no production defect and makes no
performance claim. Production code is unchanged.

The fixture commits one action and claims its first lease at logical time 100
(expiry 110). A second claim observes time 111. The acknowledgement carries time
105, so its initial expiry check alone cannot reject it: the durable envelope
check inside the write transaction must decide whether it still owns the action.

The test first constructs both serial reference histories on separate databases:

- Acknowledgement first: acknowledgement succeeds and reclaim returns no action.
- Reclaim first: reclaim returns one replacement lease with a distinct token and
  delivery attempt 1; acknowledgement of the old lease returns `Conflict`.

It then starts both operations together through independent store connections,
using a barrier and alternating yield hints. Each race must match one complete
reference history: both operation results and every row in the ten process and
lease tables, including the token counter. Yield hints do not guarantee which
order wins. The test does not require both orders to occur in the races; both
orders are exercised explicitly by the serial references.

Every fixture closes both stores and reopens the database. If reclaim won, the
old lease still fails and the replacement lease successfully acknowledges the
same action. A final claim returns no work in either history.

Validation on stable Rust and Rust 1.85.0:

- All 62 process integration tests passed on each toolchain.
- Twenty additional test repetitions per toolchain passed: 640 raced histories
  (1,280 concurrent operations), plus 80 serial reference histories.
- A temporary negative control omitted the durable lease validation from outbox
  acknowledgement. The new test failed because the stale acknowledgement
  incorrectly succeeded. Production source was restored before the repetitions.
- Workspace strict Clippy, formatting, and layer checks passed. Graphify was
  refreshed using AST extraction.

[Logs and source hashes](verification-data/2026-10-31-outbox-ack-reclaim/manifest.json)
record the qualification. These tests cover one action and two connections;
they do not establish arbitrary-load fairness or power-loss durability.
