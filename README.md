# Penelope

Penelope is a Rust library for deterministic orchestration of long-running,
multi-step processes. It is the **process-truth** layer: it records what a
workflow decided and what its steps observed. It is not a ledger, matching
engine, market-data system, or canonical inventory/position store.

> Status: **scaffold only; not production-ready.** The workspace compiles, but
> contains no domain implementation, port traits, tests, adapters, benchmarks,
> CI, or release process. The complete execution plan is [TODO.md](TODO.md).

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

| Crate | Intended responsibility | Current state |
| --- | --- | --- |
| `penelope` | Pure domain model, definition validation, replay, transitions, compensation planning. | Module scaffold only. |
| `penelope-ports` | Backend-neutral persistence, worker, timer, inbox/outbox, command and review traits. | Module scaffold only. |

Adapters must be separate crates or live in the application composition root.
They must never be required to use or test the pure engine.

## Verification today

The following currently pass but are only compile/lint checks: the test command
runs **zero tests** because no behavior exists yet.

```bash
cargo fmt --all --check
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --locked
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
