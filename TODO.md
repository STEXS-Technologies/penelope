# Penelope release checklist

This checklist defines the finished scope of Penelope as a reusable Rust
library. Penelope is deliberately library-only: databases, brokers, schedulers,
workers, HTTP/RPC transports, deployment manifests, and StateChronicle
implementations belong to the consuming application.

## Library work — complete

The following items are implemented and covered by tests, examples, or CI
checks. They are retained here as an auditable definition of done.

- [x] Hexagonal crate boundaries with re-export-only `lib.rs` roots and small
  public entry modules.
- [x] Validated newtype identities for process, definition, step, action,
  event, effect, correlation, attempt, and idempotency values.
- [x] Typed protocol commands, inputs, events, outcomes, errors, and decisions;
  no embedded DTO wire-version or schema discriminator fields.
- [x] Canonical bytes and digests, semantic `DefinitionVersion`, and explicit
  definition migration checks.
- [x] Bounded append-only outcome-log model with ordering, sequence, duplicate,
  and integrity validation.
- [x] Deterministic linear and graph saga decision engines with ordered replay.
- [x] Idempotent effect keys, stale-result rejection, duplicate delivery
  handling, and deterministic decision replay.
- [x] Retry policy with bounded attempts, backoff, jitter, deadlines, and
  timer decisions.
- [x] LIFO compensation, cancellation, unknown-effect escalation, and typed
  manual-review decisions.
- [x] Typed ports for outcomes, inbox/outbox, leases, timers, external effects,
  reconciliation, authorization, quotas, diagnostics, and review queues.
- [x] Exact typed correlation boundary for StateChronicle commands and results;
  no substring or error-message matching.
- [x] Intent parsing, validation, constructors, ergonomic facade examples, and
  rustdoc guidance for the common async loop.
- [x] Property, adversarial, fault-injection, parser, DTO, engine, and replay
  fuzz targets. CI runs bounded smoke fuzzing; extended campaigns are run
  separately and recorded as release evidence.
- [x] CI gates for Rust 1.85 MSRV and stable, formatting, tests, Clippy with
  warnings denied, rustdoc warnings, dependency policy, action pinning, layer
  boundaries, and secret scanning.
- [x] MIT/Apache licensing, security policy, lockfile, changelog, contribution
  guidance, and reproducible release commands.

## Consumer-owned work — intentionally outside this repository

These are required of each deployment, but implementing them here would turn
Penelope into infrastructure and couple it to a storage or transport vendor.

1. Implement `penelope-ports` adapters with transactions, append-only writes,
   compare-and-swap, unique constraints, lease fencing, durable timers, and
   crash recovery.
2. Run at-least-once outbox/inbox workers and broker integrations; preserve
   effect keys and never treat a transport acknowledgement as business success.
3. Reconcile ambiguous external effects against the authoritative system before
   retrying or compensating; escalate irreconcilable outcomes for review.
4. Operate retention, backups, encryption, access control, metrics, tracing,
   alerting, rate limits, and deployment/runbooks appropriate to the product.
5. Add application-owned transport envelopes and compatibility/version policy
   when messages cross independently deployed services.

## Release procedure

Run from the repository root:

```text
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo clippy --workspace --all-targets --all-features --locked --exclude penelope-fuzz -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --locked --exclude penelope-fuzz
cargo audit
cargo deny check
./scripts/check-action-pins.sh
./scripts/check-layer-boundaries.sh
./scripts/check-no-secrets.sh
git diff --check
```

For a release, review `CHANGELOG.md`, create an annotated tag, publish the
library crates, and attach the CI run plus extended fuzz report. Repeat the
fuzz campaign after changes to parsers, protocol validation, replay, or retry
logic.
