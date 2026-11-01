# Rewindable computation examples

These two executable consumers use the same `penelope-timeline` API with independent
domain types. No game engine, ECS, network transport or StateChronicle is required.

```bash
cargo run -p penelope-timeline --example rewindable_simulation --locked
cargo run -p penelope-sqlite --example optimistic_timeline --locked
cargo test -p penelope-timeline --example rewindable_simulation --locked
cargo test -p penelope-sqlite --example optimistic_timeline --locked
```

Both examples run in stable CI and their acceptance tests run with the workspace's
all-target checks, including Rust 1.85.0. They assert their contracts and return typed
errors for model, timeline and adapter failures. They are examples of consumer
composition; their invoice/kinematics models are not exported library contracts.

## Integer simulation

`crates/penelope-timeline/examples/rewindable_simulation.rs` owns a complete integer
`World { position, velocity }` and captured `Movement { acceleration, wind }` inputs.
The library supplies metadata, fixed history, replay, validation and publication.
A separate closed-form i128 oracle checks the sequential i64 model's result.

The example:

1. Appends and executes six live inputs, capturing complete snapshots.
2. Imports an authenticated post-step S2, restores scratch and replays I3..I6,
   yielding after each step while the live publication remains unchanged.
3. Replaces input I4 on a child branch with a higher immutable input revision,
   restores exactly S3 and replays I4..I6, including a zero-budget yield.
4. Validates each candidate against the independent oracle and promotes the
   state/head together. Original branch snapshots remain readable.
5. Expires history through a complete S3 floor and materializes the selected
   branch, retaining the corrected input version while reclaiming old ancestry.

Wind is an explicit input; there is no ambient RNG or clock. A consumer that adds
randomness, configuration or other external observations must capture them too.

## Optimistic invoice and durable handoff

`crates/penelope-sqlite/examples/optimistic_timeline.rs` owns an `Account`, `Purchase`
and `Invoice`; it shares no simulation types. Its predicted total is 46. A delayed,
application-authenticated post-first-purchase total of 18 produces 44 after replay.
The old tentative invoice is discarded. Observation and local promotion leave
irreversible-effect finality unset. A separate application authorization finalizes
the prefix before `EligibleEffect` can cross `EffectHandoff`.

The composition has three local durable SQLite files:

| File | Owner and contract |
| --- | --- |
| `commands.db` | Application-owned immutable scoped command bytes and timeline origin; fixed 16-byte invoice payload and 245-byte v1 intent metadata; at most 16 commands |
| `process.db` | Existing Penelope SQLite adapter: one v0.3 `AtomicProcessCommit` atomically admits an authorized manual-resolution input, outcome and external-effect outbox action |
| `destination.db` | Example cooperating endpoint: scoped idempotency key, immutable binding and invoice are committed in one transaction; at most 16 receipts |

The source process is derived from the full timeline scope and stable effect ID.
The dedup key excludes origin revision/branch; the content binding includes both,
the full v1 intent metadata and payload. Reusing a key with changed origin or bytes
is refused. Definition/model/schema and payload digest bindings are checked before
admission and dispatch. No new saga/task input kind or wire variant is introduced:
the sample's explicit operator authorization is an existing `ManualResolution`.
The source action is an existing `ExternalEffect` with its normal v0.3 effect key.

Command bytes are made immutable and durable **before** the source transaction.
A failed source commit can leave one bounded orphan command; it cannot leave a
committed action referencing bytes that were never stored. The example does not
automatically delete these bytes, because a retained outbox action may need them.
Source commits use one stable sequence-zero process per scoped effect. An exact
retry verifies the authoritative committed batch in the existing SQLite adapter
and the returned outcome page, rather than appending another outcome or guessing
that an ACK failure means the action was absent.

Two explicit failures are exercised:

1. The source commit succeeds, but the handoff returns an injected unavailable ACK.
   The timeline keeps the same eligible intent. Reopening the adapters and retrying
   reconstructs the identical commit and leaves one outcome and one outbox action.
2. The destination commits the invoice, but its ACK is lost. The source lease is
   left unacknowledged. After adapter/destination reopen and injected lease expiry,
   the same action is redelivered. The destination recognizes its durable scoped
   key and immutable binding; one invoice remains. Only then is the source lease
   acknowledged. A contradictory correction across finality is refused.

The sample also tests command quotas, wrong scope, changed origin/payload, corrupted
bytes, substituted action identity and source-transaction rollback. A failed atomic
commit leaves no inbox, outcome or outbox row and keeps the timeline intent.

This is idempotent delivery under the documented cooperating endpoint contract.
It does not reverse external effects or establish exactly-once delivery across
arbitrary stores/APIs. The timeline itself stays in memory during the injected
ACK failures: reopening the durable handoff adapters is not a claim of a complete
persistent `LiveTimeline` recovery image. A production consumer must persist its
matching state/head, consumed IDs, pending intents and finality authorization under
its chosen recovery/publication transaction. The application-owned command and
endpoint tables are demonstration composition, not additions to Penelope's schema.
All database work happens outside replay; the sample's blocking waiter belongs at
a synchronous composition root. Its injected lease time is separate from timeline
steps and revisions.

## What downstream prediction integrations still supply

The consumer owns authenticated authority and finality, a complete consistency
domain, snapshot representation and migration, deterministic computation and
captured nondeterminism, input transport and ordering, buffering/resynchronization
policy, and supported atomic publication/recovery. Hashes only check content.

A game-engine adapter additionally supplies fixed-step scheduling, complete
component/resource/entity lifecycle capture, deterministic physics/RNG, input and
snapshot replication with acknowledgements, trust and untrusted-input validation,
cross-domain dependencies and synchronization, interpolation/visual correction,
interest management and bandwidth policy, lag compensation and resynchronization
UX. Test those integrations under actual latency, jitter and loss. These are
consumer responsibilities; the generic timeline crate does not implement them.
