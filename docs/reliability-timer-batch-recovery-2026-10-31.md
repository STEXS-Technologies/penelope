# Durable partial timer-batch recovery, 2026-10-31

The executor now has regression coverage for delivery failures partway through a real SQLite timer batch. The current production implementation passes; this change adds tests and evidence only. Runtime code, schemas, settings and performance are unchanged.

The matrix covers 17 timers with nine-byte scope/action identifiers and 128 timers with maximum-length scope/action identifiers. Each batch fails at its first, middle or last timer, across eight boundaries: before/after payload persistence, before/after inbox acceptance, before/after timer acknowledgement, clock failure after acceptance and a mismatched receipt returned after successful acceptance. These 48 cases exercise the large-batch validation path as well as delivery and storage.

## What is checked

Every case plans the batch with a public atomic process commit, schedules every timer through the public scheduler and calls `drain_timers` with injected ports that delegate to the real store. The selected port returns `Unavailable` before or after its successful durable operation; the receipt case substitutes only the returned input ID and expects `Invariant`. The test verifies that the selected fault actually fired and checks exact stage, source, claimed count, acknowledged count, duplicate count and payload/accept/ack call counts.

Raw SQLite reads independently check every timer's canonical bytes/checksum, full serialized lease, terminal state, attempt, expiry and token counter. The accepted inputs and persisted payloads must equal the exact prefix justified by the failure point. A lost response after a durable ACK deliberately has one more acknowledged timer in storage than the report can verify. The report must not claim that ambiguous acknowledgement succeeded.

After dropping the adapter handle, a newly opened adapter uses a different owner. At the original inclusive expiry boundary, no lease may be reclaimed and all ten process/lease-counter tables must retain the failed-state snapshot. One tick later, exactly the unfinished timers are reclaimed, with the expected fresh tokens, new owner, new expiry and incremented attempts. Previously acknowledged timers stay terminal. The recovery report distinguishes fresh acceptance from the one previously accepted input, when present.

Input values and canonical payload bytes must match the pinned parent builder across the owner/token/expiry changes. Every payload is read back through the public payload store. The complete pending-input set is checked against SQLite, with the public 64-item discovery prefix checked repeatedly. Timer ACK leaves those inputs pending; it does not consume them.

The host then discovers and consumes inputs through public 64-item inbox batches, so the maximum-size case must expose its second batch after the first is consumed. Each input produces one atomic `TimerFired` outcome. All commits are retried, with valid duplicate receipts and an unchanged ten-table snapshot, including quota/usage state. A second adapter reopen must preserve the same state, show no pending inputs or claimable timers, and replay exactly the initial outcome plus one outcome per timer. A new commit attempting to consume an already consumed input must return `Conflict` without changing any table.

The oracle in `tests/support/timer_input_legacy.rs` remains an exact extraction of the builder at `00b79aaf84e59ce1a74fd2c6b75bd5dc099c0650`. Production source remains identical to parent `c69cebc8594094f80fee54d5fdc9c88bb7ef3f1d`.

## Qualification and limits

Two explicit repetitions on stable Rust and two on Rust 1.85 execute 192 fault/reopen/retry cases and consume 13,920 timer inputs, with the same number of duplicate commit retries. Every repetition retains the complete 48-case grid. Final all-feature stable workspace tests excluding the fuzz package, Rust 1.85 executor tests and all-target checks, strict workspace Clippy, formatting and layer checks pass. Graph output is refreshed locally and stays untracked.

Six negative controls are rejected: report ACK success before receiving its result, skip receipt validation, let the new owner alter a redelivered input ID, reclaim at the inclusive expiry boundary, continue after ACK failure and allow a consumed input to commit again. Source-mutating scripts restore production files in `finally` blocks before final qualification; no other project build/test/graph jobs overlap those mutations.

These are returned-error and clean adapter-reopen tests. They do not kill a process, simulate power loss or inject filesystem/SQLite COMMIT faults. Existing single-timer process-death and SQLite commit-boundary tests cover different failure models. One durable input consumption is proven here; exactly-once external effects and full saga business-result reconstruction are outside this matrix. No new latency or overhead percentage is claimed.

[Logs, source provenance, fault drivers and read-only verifier](verification-data/2026-10-31-timer-batch-recovery/) retain the evidence. Run `python3 docs/verification-data/2026-10-31-timer-batch-recovery/verify.py` from this checkout. To repeat the suite, copy `repeat.py` to an empty target directory and run it from the repository root with `stable` or `msrv`. The negative-control script temporarily changes production source and must run without other project jobs. Set `PENELOPE_TIMER_BATCH_CASE=17-9-8-AckAfter` for a single selected case; full qualification explicitly clears that filter.
