# Timer cancellation versus acknowledgement

This audit adds a cross-connection regression test for timer cancellation racing
with acknowledgement. It found no production defect. Production code is
unchanged, so this change adds no runtime overhead.

The fixture commits and schedules one timer, then claims a live lease at logical
time 100. Independent SQLite store connections attempt cancellation and
acknowledgement together. Both operations use `BEGIN IMMEDIATE`; the durable
lease check excludes cancelled timers inside the acknowledgement transaction.

Two explicit serial reference histories establish the allowed outcomes:

- Cancellation first: cancellation succeeds; acknowledgement returns `Conflict`.
- Acknowledgement first: both operations succeed.

Both histories must leave exactly the same durable rows, with timer state 2
(cancelled). Every race must match one entire reference history: both operation
results and all rows in the ten process and lease tables, including quota
accounting and the token counter. A barrier starts both operations; alternating
yield hints encourage different orders without assuming either order will win.

After both connections close, a reopened store must reject acknowledgement with
the old lease. Repeated cancellation and scheduling of the same timer must not
resurrect it. Claiming at time 1000 returns no timer, and a second complete row
snapshot must remain identical.

Qualification:

- All 63 process integration tests passed on stable Rust and Rust 1.85.0.
- Twenty extra repetitions per toolchain passed: 640 raced histories (1,280
  concurrent operations), plus 80 explicit serial reference histories.
- A temporary negative control removed `AND state IN (0,1)` from the timer lease
  lookup. The new test failed when acknowledgement changed cancelled state 2 to
  acknowledged state 1. Production source was restored before qualification.
- Workspace strict Clippy, formatting, and layer checks passed; Graphify received
  an AST-only update.

[Evidence and source hashes](verification-data/2026-10-31-timer-cancel-ack/manifest.json)
record these checks. Coverage is one timer and two connections, without a claim
about arbitrary-load scheduling, process crashes, or power-loss durability.
