# Penelope release checklist

This checklist defines the finished scope of Penelope as a reusable Rust
library. Penelope is deliberately library-only: databases, brokers, schedulers,
workers, HTTP/RPC transports, deployment manifests, and StateChronicle
implementations belong to the consuming application.

## Recoverable execution (0.2.0)

### Current architecture and ownership

The 0.1.0 foundation is retained. New task history is a separate versioned
protocol rather than an extension of existing serialized saga enums.

| Crate | Existing responsibility | Recoverable execution responsibility |
| --- | --- | --- |
| `penelope-core` | Dependency-stable primitive anchor | Allocation-free task contracts, bounded projection/replay, synchronous journal/runtime ports |
| `penelope-domain` | Saga identifiers, canonical encodings, definitions and outcomes | Existing v1 wire meaning remains unchanged |
| `penelope-intent` | Parse/validate process inputs | Existing saga inbound boundary remains unchanged |
| `penelope-executor` | Linear/graph decisions, compensation and replay | Bounded supervisor; commit before dispatch, fence before recovery, reconstruct after ambiguity |
| `penelope-ports` | Stores, effects, leases, timers, reconciliation | Hosted async journal; bridge stable saga effect identities to task generations |
| `penelope-statechronicle` | Optional verified canonical-event correlation | Optional independently verified checkpoint-state correlation |
| `penelope` | Consumer facade | Re-export task decision API and supervisor |

Contracts, projection and runtime ports occupy separate core modules. The
supervisor orchestrates injected adapters; it does not implement storage,
scheduling, task isolation, cryptographic proof verification or application
state transformation. Test-only memory/filesystem journals and simulated
workers are kept outside production code. RTOS, MPU/PMP and hardware adapters
are outside this work; Penelope is platform agnostic.

### Recovery contract

1. Commit immutable input and a pinned definition before starting. Record the
   generation authorization before dispatching; adapters deduplicate exact starts.
2. Pause a checkpoint proposal until bytes and the referencing history record
   commit atomically. Readback verifies SHA-256, length and schema. Orphan bytes
   without a committed record are never recovery boundaries.
3. On failure, fence the old generation before committing the observation.
   Reconstructing a running history records interruption; replay itself never
   executes task code or manufactures success.
4. Pure tasks may restart under their declared mode. Checkpoint mode resumes
   the latest committed checkpoint, or immutable input if none exists. Corrupt
   checkpoints are rejected, never silently replaced with earlier state.
5. Interrupted tasks with external effects, and reconcile-before-restart mode,
   require independent scoped evidence. Unknown stays blocked. Completed
   evidence binds durable output; retry requires explicit authorization.
6. A new code/checkpoint schema uses a new pinned history. The application
   transforms bytes, fences the source task, and records explicit migration
   authorization plus old/new references. Penelope never transforms state.

Attempts include the initial execution. Delay and absolute deadline prevent
unbounded restarts; input/output/checkpoint bytes, checkpoint count and history
records each have explicit quotas. The pure engine accounts for lifecycle
quotas. The runtime port enforces the supplied per-generation CPU allowance.
Reconciliation that remains unknown can require application escalation; an
expired deadline never grants permission to repeat an effect.

### Deployment profiles

The portable profile is `penelope-core --no-default-features`: no heap, runtime
or I/O. Optional `serde` also supports no_std. Descriptors, records and the
projection are fixed-size; replay streams at most `max_history_records` events.
The synchronous supervisor uses two `N`-byte buffers while dispatching and one
while polling; it rejects definitions whose blob limits exceed `N`. Its owning
crate is hosted; applications can drive the core projection/ports directly for
fully no_std deployments. Storage space is adapter-specific and must cover
immutable input/output plus retained checkpoint bytes and serialized history.
The library does not assume an allocator, flash format or persistence latency
for the portable profile. No durable write or hash is required per control tick.

### Implemented gates

- [x] Preserve existing linear/graph, compensation, reconciliation and v1 fixtures.
- [x] Explicit recovery modes, task generations, fault kinds and execution quotas.
- [x] Independent task history version and immutable task wire fixture.
- [x] Commit input/results/checkpoints before they become authoritative facts.
- [x] Byte integrity/schema verification and explicit checkpoint migration.
- [x] Runtime/storage agnostic synchronous ports and hosted async journal contract.
- [x] Optional StateChronicle checkpoint evidence correlation, no required client.
- [x] no_std/no_alloc core and CI build on a target without a hosted standard library.
- [x] Generation/scope rejection, deadline enforcement and bounded restart tests.
- [x] Inject unavailable/full storage, wrong/lost receipts and checkpoint corruption.
- [x] Kill a real process before/after blob persistence and after history commit.
- [x] Kill after an independently recorded external effect; require reconciliation.
- [x] Seeded recovery chaos and history/checkpoint fuzz targets in existing drills.

### Before tagging 0.2.0

- [x] Documented public protocol, effect-boundary and adapter security review;
      frozen full-definition fingerprint and strict task wire decoding.
- [x] Extended fuzz/chaos release campaign with retained summary/seed artifacts
      and coverage review; see `ci/recoverable-execution-campaign.json`.
- [x] Specify consumer adapter conformance: transactions/CAS, durable retention,
      idempotent start, old-generation fencing and independent effect evidence.
      Production adapter certification remains deployment-owned.
- [x] Workspace version/dependency bump and release notes for 0.2.0.
- [x] Final candidate package verification and repeated security campaign.

The first final source revision `907f340` passes 135 workspace tests on Rust 1.99.0 and
1.85, 3.3 million sanitizer fuzz runs across eleven targets, three seeded chaos
iterations with 10,000 property cases per test, and actual process-kill tests.
Line coverage is 94.3223%, above the unchanged 94.0759% minimum. All seven
0.2.0 archives pass Cargo verification using its temporary registry.
Dependency, secret, layer, no_std, Clippy and rustdoc checks pass. Seeds and
machine-readable results are retained in the linked campaign artifact.

The second deep audit adds terminal history-slot reservation, fail-closed live
quota handling, forwarded-observation timeout enforcement and persistent
runtime fencing requirements. Its full results supersede that earlier snapshot;
see the release guide and updated machine-readable campaign artifact.

Process-kill tests exercise actual host process interruption with fsync/rename.
They do not certify power-loss behavior, a database implementation, storage
media, isolation, or domain correctness. These remain consuming applications'
responsibilities. No 0.2.0 tag is implied by the completed library gates above.

### Development validation, 2026-10-10

- Full workspace tests, including unchanged saga replay/wire compatibility.
- Three complete pure chaos iterations, each with 2,000 property cases and
  at least 1,000 runs per fuzz target across all eleven registered targets.
- Additional 10,000-run campaigns for each new task history/checkpoint fuzzer.
- Process kill at four actual host boundaries, including lost effect acknowledgement.
- Rust 1.85 checks and executor/StateChronicle tests; warning-free Clippy/rustdoc.
- no_std cross-check on `riscv32imafc-unknown-none-elf`, with and without Serde.
- Runnable example: generation 2 completes from record-offset checkpoint 8.

Final candidate validation supersedes the initial development campaign.
Exact revision, reproducible commands, seeds, execution counts and coverage
are retained in [the campaign artifact](ci/recoverable-execution-campaign.json).
The public protocol/effect-boundary review is documented in
[the release guide](docs/RELEASE.md#task-protocol-and-security-review).

The review closes scope binding, early quota admission, regressed clocks and
time-overflow gaps with regression tests. Full-definition fingerprints pin all
permissions and quotas; strict new wire decoding rejects unrecognized fields.
Journal record integrity is explicit, with valid-JSON corruption rejection
tested separately from blob integrity. No external audit is claimed.

All requested library capability areas have implementation and test evidence.
Durability, isolation, CPU enforcement and authoritative external evidence
remain consumer-owned. The portable allocation-free profile is the task
decision core. No tag/publication was performed.

## Completed library work (0.1.0)

The following items are implemented and covered by tests, examples, or CI
checks. They are retained here as an auditable definition of done.

- [x] Hexagonal crate boundaries with re-export-only `lib.rs` roots and small
  public entry modules.
- [x] Validated newtype identities for process, definition, step, action,
  event, effect, correlation, attempt, and idempotency values.
- [x] Typed saga v1 protocol commands, inputs, events, outcomes, errors, and
  decisions; existing saga DTOs have no embedded wire-version discriminators.
  Independent task history uses its own explicit version and checkpoint schema.
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

## Consumer-owned work, intentionally outside this repository

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
