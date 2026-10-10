# Penelope

Penelope provides deterministic saga orchestration and recoverable task
execution for Rust. It coordinates workflows and declared computations that
must survive crashes, retries, duplicate delivery, timeouts and uncertain
external results.

Use it for settlement, trading, approvals, inventory, marketplace orders,
payments, provisioning, refunds, and other multi-step processes where “run it
again” is not a safe recovery strategy.
Recoverable tasks also support structured computations such as data ingestion,
index construction, code analysis and batch processing, with application-defined
checkpoints and explicit recovery permissions.

## What Penelope gives you

- Deterministic linear and graph saga decisions.
- Append-only outcome contracts and ordered replay after restart.
- Typed identities and scopes that prevent cross-process or stale results.
- Bounded retries, backoff, jitter, deadlines, and timer decisions.
- LIFO compensation, cancellation, reconciliation, and manual review.
- Recoverable task supervision, committed checkpoints, generation fencing and
  bounded execution budgets through injected journal/runtime ports.
- An allocation-free `no_std` decision core with optional Serde support.
- Backend-neutral ports for stores, inbox/outbox, leases, timers, effects, and
  operator workflows.

The engine is pure and deterministic. Your application supplies the durable
adapters and workers through the ports. Penelope never guesses whether an
ambiguous payment, trade, or canonical mutation succeeded.

## The programming model

Given a saga definition, replayed projection and typed input, ask the pure engine
for its next decision:

```rust
let decision = penelope::decide(
    &definition,
    projection.as_ref(),
    tenant_id,
    process_id,
    &input,
)?;
```

Build an `AtomicProcessCommit` containing the accepted input, ordered outcome
plan and pending actions, and persist it through your `ProcessStore` adapter.
Dispatch only after a validated commit receipt. Route timer and manual-review
decisions through their corresponding ports. The
[trade example](crates/penelope/examples/trade.rs) demonstrates decision and
outcome planning; application adapters provide durable commit and dispatch.

After a crash, load the ordered outcome log and replay it. Duplicate inputs are
deduplicated, stale results are rejected, and unknown effects wait for
reconciliation instead of triggering blind retries.

## Recoverable task execution (0.2.0)

The existing saga APIs and v1 records retain their meaning. Alongside them,
`penelope::task` provides explicit recovery permissions, immutable input,
committed checkpoint references, ordered task history, typed faults, bounded
recovery attempts and generation fencing. `TaskSupervisor` coordinates those
decisions with application-supplied journal and runtime ports.
Task identities, generations, history sequences and monotonic timestamps use
distinct newtypes, preventing accidental substitution across these boundaries.

Penelope never infers completion from a crash or timeout. Interrupted operations
with external effects require independent reconciliation; unknown evidence does
not authorize a restart. Application logic defines valid checkpoints, effect
boundaries and result correctness. There is no instruction-level continuation,
scheduler, OS integration or storage implementation in the library.

The `penelope-core` task contracts, projection and synchronous ports compile
without `std` or `alloc`. Select that crate directly for the portable decision
profile; the existing facade, saga crates and async ports remain hosted.

```bash
cargo run -p penelope-executor --example recoverable_task --locked
cargo check -p penelope-core --no-default-features --target riscv32imafc-unknown-none-elf --locked
PENELOPE_CHAOS_ITERATIONS=3 PENELOPE_CHAOS_PROPTEST_CASES=2000 PENELOPE_CHAOS_FUZZ_RUNS=10000 ./scripts/run_pure_chaos_drill.sh
python3 scripts/run_property_stress.py --seconds 300 --cases 10000 --jobs 3
```

Task histories have their own version discriminator, independent of saga v1.
Checkpoint schema/code changes require an explicit application transformation
into a newly pinned history; old bytes are never silently reinterpreted.
See [the architecture and release gates](docs/RELEASE.md).
Validation includes process-kill fault injection, concurrent supervisor
arbitration, stateful supervisor fuzzing and reproducible chaos campaigns.

## StateChronicle boundary

`penelope-statechronicle` provides the typed contract for canonical commands
and verified committed events. StateChronicle remains the source of truth for
balances, positions, inventory, ownership, and settlement. Penelope advances a
saga only after the matching committed event is correlated by tenant, action,
operation, resource scope, and result evidence.

Penelope and StateChronicle can use separate datastores. Correctness comes from
durable local records, at-least-once delivery, idempotency, and reconciliation.
Task checkpoint-state verification is optional: evidence binds the exact task
scope, generation and checkpoint identity, and the adapter verifies the proof.
Recoverable execution also works with an independent journal.

Custom StateChronicle domains require explicit schema migration and do not
automatically provide transactions across resources. Existing history encodings
and exhaustive Rust enum matches have distinct compatibility requirements.
See the [integration boundaries](docs/RELEASE.md#integration-boundaries) for
durability, task safety and portable-versus-hosted responsibilities. The SQLite
recovery fixture is a dev-dependency used only by Penelope's own tests; it does
not enter consumer debug or release builds.

## Start building

Add the facade crate to your application:

```toml
[dependencies]
penelope = "0.2.0"
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
reviewed integration crate. See [docs/RELEASE.md](docs/RELEASE.md) for the library
boundary, `0.2.0` release procedure and protocol review. This branch prepares
0.2.0; registry availability follows publication.

Use the [pull request template](.github/pull_request_template.md) when proposing
changes, and follow the [validation commands](docs/RELEASE.md#validation).

The project is dual-licensed under [MIT](LICENSE-MIT) or
[Apache 2.0](LICENSE-APACHE).
