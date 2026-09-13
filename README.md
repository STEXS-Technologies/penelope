# Penelope

Penelope is a Rust library for deterministic orchestration of long-running,
multi-step processes. It is the **process-truth** layer: it records what a
workflow decided and what its steps observed. It is not a ledger, matching
engine, market-data system, or canonical inventory/position store.

> Status: **early implementation; not production-ready.** Penelope has typed
> versioned protocol DTOs and a tested pure linear-saga reference engine with
> ordered-event replay. It
> does not yet have an append-only outcome implementation, durable adapters,
> complete replay/compensation semantics, benchmarks, chaos evidence, CI, or a
> release process. The complete execution plan is [TODO.md](TODO.md).

## Boundary with StateChronicle

StateChronicle owns canonical facts such as balances, reservations, positions,
inventory, ownership and settlement. Penelope coordinates the workflow around
those facts: approvals, timers, retries, external side effects, correlation,
compensation and operator escalation.

```text
event / timer / operator input
             |
             v
 Penelope decision (pure and deterministic)
             |
             +--> append outcome + update process projection
             +--> durable actions: execute | schedule | publish | escalate
             +--> idempotent canonical command to StateChronicle
                                                        |
                                                        v
                                               committed canonical event
                                                        |
                                                        +--> Penelope inbox
```

The core invariant is:

```text
projection = apply(definition_version, ordered_outcome_log)
```

An effect result is recorded as an outcome and is never recreated merely by
replay. The core contains no I/O or wall-clock reads; adapters do I/O.

An action result is accepted only when its typed action ID equals the active
action ID in the projection. A stale, duplicate, or cross-process result cannot
advance a process; duplicate delivery must be handled by the durable outcome
layer that is still to be implemented.

The reference ordered replay API accepts only zero-based contiguous event
envelopes. A missing, duplicate, or reordered sequence is rejected before
event semantics are applied.

## Reliability model: record, recover, reconcile, repair

Penelope is designed to make asynchronous sagas recoverable, not magical.
Before an effect is attempted, it durably records the planned action and its
stable action ID. Every input, decision, timer, attempt, response, observed
canonical event, retry, compensation, review, and terminal state is appended
to the process outcome log. A restart replays that log and resumes only actions
whose record proves they remain pending.

There are three different operations that must never be conflated:

| Operation | What may change | What must never happen |
| --- | --- | --- |
| Replay / rewind | Rebuild Penelope's derived process projection from its immutable outcome log. | Reissue an old effect merely because it was replayed. |
| Reconciliation / self-healing | Resume a known safe retry or query durable evidence for an ambiguous action. | Guess that an unknown payment, trade, or settlement failed. |
| Repair / compensation | Append a new authorized process decision and submit a new canonical compensating command. | Rewrite or delete StateChronicle history or directly overwrite canonical state. |

Unknown external outcomes are not retryable by default. Penelope must first
reconcile the action ID with the external system and, for canonical mutations,
with StateChronicle's committed history. If evidence remains ambiguous, it
opens manual review with the complete process evidence.

## StateChronicle integration contract

The `penelope-statechronicle` workspace crate is the dedicated future adapter
boundary. It must implement this protocol exactly:

1. In one Penelope transaction, append the decision, create a canonical-command
   action with a stable action/command ID, and enqueue dispatch.
2. Submit that command through a verified StateChronicle durable API with the
   exact same idempotency ID. A network response alone is not success.
3. Consume the StateChronicle transactional-outbox notification through a
   durable Penelope inbox keyed by its immutable delivery/event identity.
4. Verify the notification is a committed canonical result for the expected
   tenant, command/action ID, resource scope, and transition. Only then append
   the Penelope `CommandCommitted` outcome and advance the saga.
5. On restart, replay the Penelope log. Pending commands are reconciled by ID;
   no action is resubmitted until its current StateChronicle result is known.

Penelope and StateChronicle may use separate datastores. They therefore do not
form a distributed transaction: correctness comes from durable local records,
at-least-once delivery, idempotency, committed-event correlation, and explicit
reconciliation. Penelope must use a narrowly authorized service principal and
may issue only the operations declared by the pinned process definition.

## Intended capabilities

- Immutable, versioned definitions pinned when an instance starts.
- Append-only outcome log and deterministic replayable projection.
- Idempotent effects keyed by tenant, instance, step and attempt.
- Durable inbox deduplication, outbox publication and timer scheduling.
- Bounded retries, deadlines, error classification and cancellation.
- LIFO, exactly-once compensation plus audited manual review.
- Tenant isolation, authorization context, quotas, redaction and telemetry.

## Appropriate economic and trading workflows

Penelope may coordinate multi-party settlement, venue-order lifecycle, risk
approval, margin-call/liquidation, reconciliation, corporate actions, game
market listing/purchase/delivery/refund, and exception handling. It must not
make a canonical mutation itself: every balance, position, inventory, or order
state change is an independently idempotent StateChronicle command. The
workflow advances only after the matching committed event is received.

It cannot provide exactly-once network delivery. Its safety model is
at-least-once delivery plus durable outcomes, idempotency keys, reconciliation
of ambiguous external effects, and manual escalation when ambiguity remains.

## Workspace

Penelope follows StateChronicle's workspace-first hexagonal layout. Dependency
arrows point inward only; a pure inner crate cannot depend on a port, adapter,
database, broker, clock, transport, or local checkout.

```text
transport / database / broker / scheduler implementations (consumer-owned)
                              |
                              v
                 penelope-statechronicle  [adapter boundary only]
                              |
                              v
                   penelope-ports         [interfaces]
                              |
                              v
                 penelope-executor        [application]
                              |
                              v
                  penelope-intent         [inbound validation]
                              |
                              v
                  penelope-domain         [versioned DTOs]
                              |
                              v
                   penelope-core          [schema primitives]

                  penelope                [umbrella facade]
```

| Crate | Responsibility | Current state |
| --- | --- | --- |
| `penelope-core` | Pure schema constants and shared protocol primitives. | Versioned schema IDs only. |
| `penelope-domain` | Versioned public DTOs for definitions, inputs, outcomes, actions, canonical commands/events and review. | DTOs only; no workflow logic. |
| `penelope-intent` | Transport-to-domain validation boundary. | Contract scaffold only. |
| `penelope-executor` | Application-layer deterministic decision/replay composition over injected ports. | Pure linear-saga reference engine: ordered steps, typed event replay, replayable projection, typed retry attempts, completion and safe escalation on unknown outcomes. |
| `penelope-ports` | Backend-neutral process store, inbox, action, timer, canonical-state and review interfaces. | Interfaces only; no implementation. |
| `penelope-statechronicle` | Outer adapter boundary for verified durable commands and committed-event correlation. | Typed tenant/action/operation/resource-scope verifier; intentionally no StateChronicle client or local-checkout dependency. |
| `penelope` | Consumer umbrella facade re-exporting all architectural layers. | Facade only. |

All DTOs are versioned by their `V<N>` Rust type and immutable associated
`SCHEMA` identity, such as `ProcessOutcomeDtoV1::SCHEMA`. New wire changes
require a new DTO/schema version; no existing version may be reinterpreted.
Every identity is a validated prefixed newtype, every category is a typed enum,
and port APIs accept typed values only—application code never dispatches by
matching raw strings. Infrastructure implementations must live in a consumer
composition root or a separately reviewed adapter repository; this workspace
deliberately ships none.

Errors are typed `thiserror` enums. Error variants communicate a stable failure
class; they do not expose handwritten `Display`/`Error` implementations or use
raw text as a programmatic error discriminator.

The reference engine gives every step an explicit typed maximum attempt count.
An exhausted retryable failure escalates without producing another action;
unknown outcomes escalate immediately. Deadline/backoff/timer policy still
belongs to the remaining P0 implementation work.

Every planned action ID is retained in the replayable process projection. A
transition that tries to reuse any previously issued ID is rejected as a typed
engine error; retries require a fresh action identity.

For a known terminal forward failure, the reference engine plans declared
compensations in reverse success order. An unknown forward or compensation
result never triggers compensation automatically and escalates instead.

Public parsers, every versioned DTO deserializer, and the linear engine's
transition surface are covered by cargo-fuzz targets in `fuzz/`. New public
parse, DTO, or decision surfaces must add a target before they are considered
complete.

The public linear-engine `V1` definitions, projections, decisions, outcomes,
and ordered event envelopes are serde-compatible protocol values. Their schema
evolution remains additive; durable adapter design and compatibility fixtures
are still required before a production release.

## First reference saga: `trade.v1`

The first implementation is a reference for the pattern, not special ledger
logic: `validate proposal → freeze A → freeze B → await acceptance/deadline →
atomically settle → compensate/unlock or escalate`.

- Every `trade.lock`, `trade.settle`, and `trade.unlock` is a separate durable
  Penelope action with its own stable idempotency key.
- Settlement is one StateChronicle atomic batch. Penelope only compensates by
  unlock after evidence proves settlement did **not** commit.
- A timeout or lost response during settlement is `reconcile`, never `unlock`.
- A terminal failure, inconsistency, or unresolved outcome produces an audited
  manual-review case; it never silently strands or releases assets.

The compiling pure happy-path reference is
[`trade_v1.rs`](crates/penelope/examples/trade_v1.rs). Run it with:

```bash
cargo run -p penelope --example trade_v1 --locked
```

It demonstrates the public facade only. It is not a durable integration: a
production adapter must record outcomes and verify StateChronicle evidence as
described above before calling the transition API.

## Verification today

The following pass locally. They validate only the implemented protocol and
linear-engine slice; they are not production-readiness evidence.

```bash
cargo fmt --all --check
cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --locked
cargo fuzz run fuzz_identifiers -- -runs=100
cargo fuzz run fuzz_versioned_dtos -- -runs=100
cargo fuzz run fuzz_linear_engine -- -runs=100
cargo fuzz run fuzz_statechronicle_correlation -- -runs=100
cargo bench -p penelope-executor --bench linear_saga --locked
```

Do not publish or deploy Penelope until P0 and P1 in [TODO.md](TODO.md) are
complete and their CI verification exists.

## Non-goals

- Canonical financial, game, inventory or position state.
- A distributed transaction across services.
- Unbounded user-supplied workflow code running in-process.
- Replacement for authorization, risk, accounting, reconciliation, or market
  surveillance systems.

## License

The manifests declare `MIT OR Apache-2.0`. Add the complete `LICENSE-MIT` and
`LICENSE-APACHE` texts before publishing.
