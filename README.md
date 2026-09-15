# Penelope

Penelope is the foundation for reliable Rust sagas. It coordinates long-running
and asynchronous workflows that must survive crashes, retries, duplicate
delivery, timeouts, and uncertain external results.

Use it for settlement, trading, approvals, inventory, marketplace orders,
payments, provisioning, refunds, and other multi-step processes where “run it
again” is not a safe recovery strategy.

## What Penelope gives you

- Deterministic linear and graph saga decisions.
- Append-only outcome contracts and ordered replay after restart.
- Typed identities and scopes that prevent cross-process or stale results.
- Bounded retries, backoff, jitter, deadlines, and timer decisions.
- LIFO compensation, cancellation, reconciliation, and manual review.
- Backend-neutral ports for stores, inbox/outbox, leases, timers, effects, and
  operator workflows.

The engine is pure and deterministic. Your application supplies the durable
adapters and workers through the ports. Penelope never guesses whether an
ambiguous payment, trade, or canonical mutation succeeded.

## The programming model

```rust
let decision = engine.decide(&definition, &projection, input)?;
store.commit(decision.outcomes(), decision.actions())?;
for action in decision.actions() {
    outbox.dispatch(action)?;
}
```

After a crash, load the ordered outcome log and replay it. Duplicate inputs are
deduplicated, stale results are rejected, and unknown effects wait for
reconciliation instead of triggering blind retries.

## StateChronicle boundary

`penelope-statechronicle` provides the typed contract for canonical commands
and verified committed events. StateChronicle remains the source of truth for
balances, positions, inventory, ownership, and settlement. Penelope advances a
saga only after the matching committed event is correlated by tenant, action,
operation, resource scope, and result evidence.

Penelope and StateChronicle can use separate datastores. Correctness comes from
durable local records, at-least-once delivery, idempotency, and reconciliation.

## Start building

Add the facade crate to your application:

```toml
[dependencies]
penelope = "0.1.0"
```

Run the included workflows to see the public API:

```bash
cargo run -p penelope --example trade --locked
cargo run -p penelope --example trade_compensation --locked
cargo run -p penelope --example trade_retry_timer --locked
cargo run -p penelope --example trade_manual_review --locked
cargo run -p penelope --example trade_competing_lock --locked
```

## Scope and release

Penelope is a reusable library, not a database, broker, scheduler, or hosted
service. Implement those adapters in your composition root or a separately
reviewed integration crate. See [TODO.md](TODO.md) for the library boundary and
[docs/RELEASE.md](docs/RELEASE.md) for the `0.1.0` release procedure.

The project is dual-licensed under [MIT](LICENSE-MIT) or
[Apache 2.0](LICENSE-APACHE).
