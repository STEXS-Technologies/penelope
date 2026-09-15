# Penelope implementation TODO

## Current state

This repository contains an early pure-engine slice: versioned typed DTOs,
validated newtype identities, ports, a reference linear state machine, unit and
property tests, and parser/DTO/engine fuzz targets. It has no durable outcome
store, production adapter, or production-readiness evidence. Nothing below is
complete unless checked off with linked tests and reproducible evidence. Do not
call Penelope production-ready before every P0 and P1 item is complete.

## Deep audit snapshot (2026-09-15)

The current library-only gate passes: workspace tests, clippy with warnings
denied, documentation generation, dependency-layer checks, and reachable-history
secret scanning. Completed one-hour parallel fuzz campaigns also reported no
crashes or sanitizer markers. This evidence does not prove that an external
adapter preserves the contracts.

Open risks found by the deeper security, stability and reliability audit:

1. **P0/P1 — atomicity remains a consumer obligation.** `AtomicProcessStore`,
   `OutboxStore`, inbox deduplication, sequence checks and lease fencing are
   interfaces/validators only. A consumer adapter must use one transaction or
   CAS for each invariant, atomically claim ownership, guarantee unique fencing
   tokens, and reject stale acknowledgements. Add an executable adapter
   conformance suite before release.
2. **P0 — identity/canonical encoding is incomplete.** Remaining correlation,
   timer and review identity coverage, canonical byte encoding, payload-size
   limits and compile-time non-substitutability checks are still open. Typed
   length-prefixed canonical byte encoders now exist for process scopes and
   effect keys, and are fuzz-exercised. Definitions now have a stable SHA-256
   digest over canonical bytes plus a fail-closed verifier. The shared
   `CanonicalWireBytesV1` trait now covers every current domain and port DTO;
   the v1 digest migration policy remains open. Never use display text as an
   idempotency key or signature input.
3. **P1 — recovery semantics are incomplete.** Durable inbox/outcome replay,
   timer claiming, cancellation, compensation ordering and ambiguous-effect
   reconciliation lack a complete portable contract and adversarial state-machine
   tests. A bounded, atomic in-memory `OutcomeLogV1` validator now centralizes
   scope/order/duplicate checks, but persistence and restart behavior remain
   adapter responsibilities. Only the implemented reference transitions are
   currently restart-safe.
4. **P1 — authorization/redaction are incomplete.** Fail-closed authorization
   helpers do not yet define a complete operation/resource matrix or durable
   quota accounting. A typed `QuotaRequestV1` now bounds typed resource classes
   and admission units. `ProcessAuthorizationOperationV1` now publishes a
   minimum control matrix, requiring distinct principals for review decisions
   and terminal overrides. A typed `RedactedDiagnosticV1` carries only a coarse class,
   bounded metadata size and an evidence digest—never a raw message or payload—
   with tests for bounds and error classification. Consumers must deny by default
   and avoid logging raw payloads.
5. **P1 — definition evolution is incomplete.** Pinning and digest comparison
   exist, and a typed compatibility classifier now distinguishes exact,
   migration-required and incompatible definitions with a fail-closed exact
   requirement and tests. `DefinitionMigrationV1` now binds a same-definition
   source/destination version and digest and rejects no-op or cross-identity
   migrations. Registration and migration execution rules remain open; changed
   definitions must be rejected during deployment unless a consumer supplies an
   explicit migration policy.
6. **P3 — performance evidence is workload-specific.** Current host runs measure
   about 0.15M graph ops/s, 13.8M linear ops/s and 1.88M isolated graph ops/s across
   32 workers. These exclude serialization, persistence, contention, scheduling
   and network latency; they are not durable end-to-end throughput claims.

Release decision: **not production-ready** until the open P0/P1 contracts have
executable evidence in the library and consumer adapter conformance suites.

## Priority order

| Priority | Outcome | Why it precedes later work |
| --- | --- | --- |
| P0 | Deterministic domain engine | Without replay-safe behavior, adapters only persist bugs. |
| P1 | Durable contracts and integration safety | A workflow must survive crashes, retries and duplicate delivery. |
| P2 | Reference adapters and operational controls | Users need proven persistence and recovery patterns. |
| P3 | Performance, chaos and ecosystem maturity | Scale claims are meaningful only after safety is established. |

## Product contract to build

Penelope is the durable process-truth engine for sagas and multi-step async
events. It must record every causally relevant fact before or when it is acted
upon: input receipt, decision, planned action, attempt, response, timer,
committed canonical event, retry, compensation, review and terminal result.

Its recovery contract is strict:

```text
replay outcome log -> rebuild process projection -> identify pending action
  -> reconcile durable external/canonical evidence by action ID
  -> retry only if policy and evidence permit
  -> otherwise escalate with immutable evidence
```

Rewind is replay of Penelope's derived projection only. StateChronicle events
and commits are immutable: a canonical correction is a new authorized,
idempotent compensating or repair command, never history rewrite.

## Hexagonal workspace contract

The workspace now mirrors StateChronicle's layer boundaries:

| Layer | Crate | Permitted contents | Forbidden contents |
| --- | --- | --- | --- |
| Core | `penelope-core` | Schema constants and pure protocol primitives. | I/O, runtime, ports, adapters. |
| Domain | `penelope-domain` | Versioned data-only DTOs. | Database/transport types and workflow side effects. |
| Intent | `penelope-intent` | Transport-to-domain parsing and validation. | HTTP/broker/database clients. |
| Executor | `penelope-executor` | Deterministic application decisions using injected ports. | Infrastructure implementation. |
| Ports | `penelope-ports` | Async backend-neutral interfaces and port errors. | Storage, worker, transport or client implementation. |
| Adapter boundary | `penelope-statechronicle` | StateChronicle mapping contract. | Local path dependency or actual StateChronicle client/database implementation. |
| Facade | `penelope` | Curated re-exports only. | Domain or infrastructure logic. |

Every public wire DTO must have both a `V<N>` Rust type and an immutable
associated schema identifier. Every identity must be a validated newtype,
every protocol category must be an enum or dedicated newtype, and ports must
never receive raw `String`/`&str` identity or category values. Compatibility is
additive: a new semantic interpretation requires a new schema/type, while
adapters accept only versions they explicitly support.

All errors must be `thiserror::Error` enums with typed variants. Do not hand
write `Display` or `std::error::Error`, and do not make callers branch on an
error message string.

### P0.0 Complete the hexagonal contract tests

- [ ] Add compile-time dependency-boundary checks and DTO schema-version
  validation plus a fuzz target for every public parser/DTO boundary.
- Why: a clean directory tree is not architecture if an inner crate can import
  an outer implementation or an adapter can silently reinterpret a DTO.
- How: keep infrastructure crates out of this workspace; add forbidden
  dependency checks, DTO fixture/round-trip tests, and a compatibility matrix
  for each supported schema version.
- Evidence: CI rejects boundary violations, unknown schema versions, changed
  v1 fixtures, non-versioned public wire types, raw-string port parameters and
  missing fuzz targets. The normal workspace suite excludes `penelope-fuzz`;
  CI invokes every fuzz target explicitly with a bounded run count.

Current partial evidence: `scripts/check-layer-boundaries.sh` validates the
exact direct internal dependency graph through `cargo metadata` and rejects
known production infrastructure clients from this ports-only workspace. CI runs
it before compilation. `ProcessInputEnvelopeV1` carries an explicit typed
schema discriminator, and the intent boundary parses byte input only after
validating that discriminator; both its parser and DTO boundary are fuzzed. It
uses canonical versioned wire discriminators (for example,
`penelope.process.input.v1`) rather than Rust enum names; fixture tests reject
unknown and unversioned discriminators. Every current public wire DTO now
carries and validates its immutable `SchemaV1` discriminator, including
definition, input, outcome, action, canonical command/event, and manual review;
negative tests cover a mismatched schema for each and the versioned-DTO fuzz
target invokes each available validator. `scripts/run_bounded_fuzz.sh` verifies
the required target registration/source set and that every current public
versioned DTO appears in that DTO fuzz boundary before executing the bounded
suite. Immutable checked-in v1 JSON fixtures now cover every current public
wire DTO and input envelope; typed tests deserialize, validate, compare against
the expected newtype DTO, and reserialize each fixture. The layer-boundary gate
additionally rejects raw `String`/`&str` parameters in public port traits and
fields in public protocol values, plus handwritten
`std::error::Error` implementations, preserving validated-newtype and
`thiserror` error-taxonomy rules as the workspace evolves. The repository also
has a fail-closed reachable-history secret-scan script, and CI runs a complete
checkout-history scan on every push and pull request.

## P0 — deterministic core

### P0.1 Stable IDs, envelopes, validation and limits

- [ ] Add opaque types for tenant, process instance, definition, definition
  version, step, attempt, event, command, timer, correlation, idempotency and
  review IDs.
- Why: raw strings/UUIDs allow cross-tenant and wrong-resource mix-ups.
- How: serde-compatible newtypes; canonical encoding; bounded non-empty
  validation; explicit tenant/principal context; conservative limits for graph
  depth, steps, outcomes, payload bytes, attempts, timers and open instances.
- Evidence: malformed-input/property tests, serialization golden tests, limit
  boundaries, and compile-time non-substitutability checks.

Current partial evidence: all current identity values are validated prefixed
newtypes with distinct typed `DomainError` variants. Definition construction
and validation reject empty, oversized, or duplicate step lists; canonical
commands/events reject oversized or duplicate resource scopes. The versioned
DTO fuzz target executes these validators after parsing. Tenant/principal
authorization context, the remaining ID categories, canonical encoding,
payload/instance/timer limits, and
compile-time non-substitutability tests remain incomplete. The linear engine
also bounds each forward or compensation step to 64 total attempts, preventing
unbounded issued-action growth from a malformed definition.

### P0.2 Immutable versioned definitions

- [ ] Implement `ProcessDefinition`, `DefinitionVersion`, `StepDefinition`,
  transitions, correlation rules, retry/timeout policy and compensation policy.
- Why: running instances must retain their original semantics after deployment.
- How: validate at registration; pin definition version and content digest at
  start; require explicit terminal states; reject ambiguous ordering and
  unbounded cycles unless a bounded-loop construct is specified.
- Evidence: tests for invalid graphs, version pinning, digest mismatch, graph
  bounds and rejected incompatible migration.

Current partial evidence: the linear reference definition, durable start event,
and its projection now pin validated `DefinitionId`, `DefinitionVersion`, and
a definition `ContentDigest`. Replay and every transition compare the supplied
definition to those pinned values and fail closed on mismatch; this is unit
tested and the changed serde definition boundary is fuzzed. Both domain and
linear definitions reject duplicate step identities and the bounded step limit.
The engine rejects retry and compensation policies above its bounded per-step
attempt limit. Graph validation, registration, compatibility/migration policy,
non-linear transitions, full digest
calculation, and deployment-time definition storage remain incomplete.

`ProcessGraphDefinitionV1` now provides a separate versioned graph contract
with bounded steps/transitions, typed result edges, explicit entry step,
bounded visit count, duplicate/reference checks, and reachability validation.
It is deliberately not accepted by the linear executor yet; graph definitions
cannot be silently coerced into vector order. A dedicated pure graph executor
now provides typed start/result events, deterministic transition selection,
replay convergence, issued-action uniqueness, bounded visits, and immediate
escalation for unknown outcomes. Its adversarial validation and execution cases
are unit tested and its serde boundary is exercised by `fuzz_linear_engine`.

### P0.3 Append-only outcomes and deterministic projections

- [ ] Implement ordered outcome envelopes with sequence, causal input, actor,
  schema version, idempotency key, timestamp and payload digest.
- [ ] Add outcomes for start, input acceptance, step lifecycle, timers,
  commands, compensation, review and terminal completion/cancel/escalation.
- Why: the outcome log is Penelope's only durable truth and must replay exactly.
- How: establish one total sequence per instance; reject illegal sequences;
  preserve observed effect results rather than recomputing them.
- Evidence: golden fixtures, schema compatibility tests, out-of-order/adversary
  tests, and `replay(log) == stored_projection` property tests.

Current partial evidence: `ProcessOutcomeDtoV1` records a pinned process and
definition scope, total sequence, causal input/action, immutable outcome ID,
typed actor, injected logical timestamp, payload digest, and explicit schema.
`AtomicProcessCommitV1` rejects non-outcome schemas before an adapter sees a
commit; malformed versioned outcome DTOs and atomic commits are fuzzed. The
linear engine now derives an ordered required outcome-kind plan from each
immutable event and pure decision (observed result plus planned action/retry,
compensation, review resolution, or terminal state). It rejects a supplied
outcome batch that omits, reorders, or substitutes those lifecycle kinds; the
start-plan contract is unit tested and engine fuzzing exercises both plan
derivation and full outcome-batch construction.
Every externally delivered action result, retry-timer fire, and manual review
resolution now also requires an `InputAccepted` fact before its observed fact.
Those immutable events carry validated `InputId` values, and the builder rejects
an accepted-input fact whose causation identifies another input.
The public `LinearSagaInputV1` boundary likewise requires a validated `InputId`
for start, result, timer, and manual-resolution commands.
Its `to_event` conversion preserves that exact ID in the immutable replay event;
start events now likewise require and record input acceptance before creation.
Replay rejects a process log that repeats any accepted input identity, providing
a second fail-closed guard in addition to the future durable inbox.
The engine now constructs the full scope-pinned, contiguous DTO batch from
outer-supplied immutable facts and rejects mismatched scopes or sequence
overflow. It still does not allocate outcome IDs/timestamps or apply these
records to a durable projection, so this item remains incomplete.

### P0.4 Pure transition and replay function

- [ ] Implement `decide(definition, projection, input) -> Decision` and
  `apply(projection, outcome) -> Result<Projection, DomainError>`.
- Why: pure deterministic logic enables audit, recovery and exhaustive tests.
- How: pass time/entropy explicitly; sort collections before decisions; prohibit
  I/O, globals and wall-clock access in the core; issue stable action IDs.
- Evidence: repeated-decision determinism tests, no-panic fuzz/property tests,
  and decision/apply consistency tests.

Current partial evidence: `penelope-executor::engine` implements a deliberately
small deterministic linear reference machine with a replayable projection,
ordered steps, typed action IDs and attempts, retry, completion, and safe
escalation for terminal or unknown outcomes. An observed result must match the
active action ID before it may advance the projection. An ordered event replay
function rebuilds only the final pending decision and has no effect-dispatch
side effect. `replay_ordered` additionally rejects a non-contiguous zero-based
event envelope sequence before applying it. Its unit/property tests and
`fuzz_linear_engine` target run in the commands documented in `README.md`.
`decide` provides one typed side-effect-free entry point for start and
correlated-result inputs, requiring an existing replayed projection for results.
All exposed linear-engine `V1` values now have serde forms, including ordered
event envelopes; the engine fuzz target deserializes those envelopes. Explicit
schema identities/fixtures and a compatibility matrix remain missing.
It is not a complete `decide`/`apply` outcome-log engine and does not satisfy
this item.

### P0.5 Step state machine and effect idempotency

- [ ] Model pending, runnable, running, waiting, retrying, succeeded, failed,
  compensating, compensated, cancelled and terminal states.
- Why: duplicate messages can otherwise double-charge, double-deliver, or
  duplicate a settlement leg.
- How: derive an effect key from tenant, instance, definition version, step,
  attempt and effect type; distinguish known failure from unknown outcome.
- Evidence: exhaustive transition table and duplicate command/event/timer/result
  property tests.

### P0.6 Retry, timeout and timer rules

- [ ] Add bounded attempts, deterministic backoff with explicit jitter,
  retryable classification, deadlines, due-time calculation and cancellation.
- Why: retries are a reliability, safety and cost boundary.
- How: persist the decision and due time before scheduling; make duplicate timer
  fire idempotent; never call system time from core logic.
- Evidence: simulated-clock tests for boundary, clock jump, fire/cancel race,
  exhausted retry and duplicate firing.

Current partial evidence: `RetryPolicyV1` bounds total per-step attempts with
`NonZeroU32`. The reference engine escalates an exhausted retryable result and
never emits a new action in that case. `RetryBackoffV1` calculates a
deterministic exponential due time from supplied logical time, caps growth, and
fails closed on invalid bounds or time overflow; it is unit tested and fuzzed
through the serialized linear definition boundary. A retryable failure under a
backoff policy now produces a typed durable timer action and due-time schedule;
the pure engine rejects ordinary action-result delivery, early firing, and
duplicate firing, and replay rebuilds the post-fire retry action exactly. There
is now optional bounded deterministic jitter. Its opaque typed seed is supplied
by the composition root and persisted in the retry scheduling input/event, so
the exact due time replays without hidden entropy; unit and fuzz paths exercise
that wire field. A typed inclusive deadline is also retained in that input/event;
the reference engine allows an exact-boundary retry but takes the existing safe
compensation-or-escalation path when the calculated due time would exceed it.
`TimerScheduleV1` now rejects non-timer or malformed actions, and the timer
port accepts that complete scope-pinned record for both scheduling and
cancellation rather than an action ID alone. There is no durable adapter
implementation, clock-jump policy, or compensation-timer support, so this item
remains incomplete.

Current partial P0.5 evidence: the projection retains all action IDs issued by
one process. A result must correlate to the active ID, and any next/retry ID
already issued by that process is rejected. This does not replace durable
inbox/outbox deduplication. Each emitted action now
also carries the pinned definition ID/version/digest alongside tenant, process,
step, attempt, kind and action ID, preventing an adapter from losing the
definition deployment context. `ProcessActionDtoV1::effect_key` derives a
typed semantic idempotency key from that pinned scope plus step, attempt, and
effect kind and payload digest; it is unit tested and exercised at the action
DTO fuzz boundary.
Durable inbox/outbox deduplication and a complete state machine remain missing,
so P0.5 remains incomplete.

### P0.7 Compensation and operator escalation

- [ ] Build a persisted LIFO compensation plan for succeeded compensable steps.
- [ ] Prevent a compensation effect running twice; route irrecoverable or
  ambiguous outcomes to an immutable manual-review action.
- Why: distributed systems cannot use one ACID transaction across services.
- How: classify irreversible steps; record each compensation result; manual
  resolution is an authorized input, never a direct projection mutation.
- Evidence: partial settlement, duplicate compensation, compensation failure,
  operator resolution and irreversible-step scenarios.

Current partial evidence: linear steps can declare a typed compensation action.
After a known terminal forward failure, the reference projection plans those
actions in LIFO order, with fresh action IDs and independently bounded retries.
Unknown outcomes and failed/exhausted compensation escalate. There is no
persisted compensation outcome schema. An authorized typed manual resolution
is now a replayable engine input: it can resume a specifically authorized retry,
start LIFO compensation, cancel, or retain escalation; direct resolution of a
non-escalated process fails closed. An escalated pure decision can also build a
full-definition-scope-pinned `ManualReviewDtoV1` for the `ManualReviewQueue` port, while a
non-escalated decision rejects the request. There is still no durable review workflow,
so this item remains incomplete. The `trade_manual_review_v1` example drills
the handoff by creating and schema-validating that request before applying the
authorized resolution.

### P0.8 Errors, authorization and redaction

- [ ] Add typed validation, conflict, sequence, definition, quota,
  authorization and adapter errors.
- [ ] Define redacted diagnostics and authorization requirements for start,
  cancel, retry, review and terminal override inputs.
- Why: orchestration is a high-value authorization boundary and a payload leak
  can expose personal or financial information.
- How: carry tenant/principal/permissions through every command; keep sensitive
  data out of debug/display/telemetry forms; make defaults bounded.
- Evidence: authorization matrix, quota, redaction snapshot and public-parser
  fuzz tests.

Current partial evidence: every library error type derives `thiserror::Error`;
no caller is expected to branch on an error message. Domain, engine, commit,
reconciliation, intent, StateChronicle-correlation, and port failures use
typed enums. `PortError` distinguishes unavailability, optimistic conflict,
unauthorized access, quota exhaustion, timeout, cancellation, ambiguity, and
adapter-invariant rejection. The authorization matrix, quotas, redacted
diagnostics, and sensitive-data snapshot tests remain incomplete. The
StateChronicle expectation constructor now rejects a canonical command whose
tenant differs from the authorized process scope (`CommandScopeMismatch`),
with an adversarial regression test and correlation fuzz coverage.
Outcome DTOs now also expose `validate_for_scope`, and atomic commits use this
single typed scope check before accepting outcomes.

### P0.9 Build `trade.v1` as the reference saga

- [ ] Implement the complete versioned reference process: validate proposal;
  lock asset A; lock asset B; wait for acceptance or deadline; atomically
  settle; compensate known non-settlement by LIFO unlock; escalate unresolved
  outcomes.
- Why: it forces the engine to prove every essential promise—correlation,
  timers, retries, atomic canonical command, compensation and manual review—on
  a real shared-inventory/economic workflow.
- How: every action receives a unique process action ID; pin the process
  definition digest; retain exact StateChronicle command/event correlation;
  distinguish `settlement_not_committed`, `settlement_committed`, and
  `settlement_unknown`. Only the first permits automatic unlock.
- Evidence: deterministic simulations covering duplicate proposals, two
  competing locks, delayed acceptance, timer duplication, crash at each action
  boundary, commit-before-response loss, delayed canonical event, partial
  evidence, compensation retry and operator resolution.

Current partial evidence: the runnable `trade_v1`, `trade_compensation_v1`,
`trade_retry_timer_v1`, `trade_manual_review_v1`, and
`trade_competing_lock_v1` examples demonstrate
pinned three-step success, known settlement failure with LIFO compensation,
persist/schedule/fire/retry timer handling, and settlement-unknown resolution
through authorized compensation. `trade_v1` additionally builds the ordered
immutable outcome records required for every happy-path event/decision and
validates the plan before advancing. The competing-lock drill proves a verified
winner can advance while a separately scoped canonically rejected lock
escalates without settlement. They use the public facade and are CI-run.
They do not model proposal validation, actual StateChronicle lock contention,
acceptance/deadline,
canonical evidence, durable commits, or full operator workflow, so this item
remains incomplete.

## P1 — durable ports and integration contract

### P1.1 Complete backend-neutral traits

- [ ] Implement documented async traits for definition/process store, inbox,
  outbox, executor, timer scheduler, canonical command submitter, review queue,
  clock, ID source, authorization and observability.
- Why: adapters cannot be reliable without precise transactional/idempotency
  contracts.
- How: `Send + Sync` request/result types include tenant, correlation and
  idempotency context; typed errors document retry/cancellation behavior.
- Evidence: fake adapters and a shared contract-test suite.

Current partial evidence: backend-neutral `Clock`, `ActionIdSource`, `OutcomeIdSource`,
`ProcessAuthorizer`, and due-time `TimerScheduler` ports now use typed logical
time, process scope, principal, action, and authorization operation values.
`ExternalEffectExecutor` now defines typed execute, reconcile, and best-effort
cancel operations over a scope-bound action/effect key, optional logical
deadline, opaque external reference identity, and explicit succeeded,
known-failure, or unknown evidence state. Its validation rejects substituted
actions or effect keys, and its unavailable test double fails closed.
Their public DTO parser boundary is fuzzed. The `ports_conformance` test owns a
test-only unavailable adapter that implements every current port, verifies
object safety and `Send + Sync`, calls every method, and proves unavailable
operations fail closed while authorization denies by default. Definition/
The process store now also defines a bounded `OutcomeReplayRequestV1`/page read
contract: every returned outcome must have the requested scope, schema, and
contiguous sequence, and continuation must be exact; request/page validators
are fuzzed. `DefinitionRegistry` now defines typed immutable-definition lookup,
idempotent registration, and explicit migration registration without any
infrastructure implementation. Definition, inbox, outbox, executor and review interfaces remain smaller
than the target contract; no authorization policy or cancellation-semantics
evidence exists yet, so this item remains incomplete. `ProcessStore::append_outcomes`
now requires the complete pinned `ProcessScopeV1` explicitly, preventing a
lax adapter from treating a sequence number alone as authorization to append
another tenant or process's outcomes. Conformance and atomic fault doubles
exercise this binding; a real adapter must enforce it transactionally.
`ProcessAuthorizationDecisionV1::require_authorized` now provides the
fail-closed conversion from a denied policy result to `PortError::Unauthorized`,
with unit coverage that does not inspect error text.

### P1.2 Atomic append/project/inbox/outbox boundary

- [ ] Define compare-and-append that atomically validates expected sequence,
  appends outcomes, updates projection, stores outgoing actions and deduplicates
  the input event.
- Why: split writes create crash windows that lose actions or repeat effects.
- How: optimistic concurrency per instance; conflict means no partial writes;
  outbox and inbox state share the durability transaction.
- Evidence: failpoints before/after every write and recovery convergence tests.

Current partial evidence: `AtomicProcessCommitV1` defines one typed local
transaction boundary for optional inbox acceptance, contiguous outcomes, and
outgoing actions. Every outcome and action carries the pinned tenant, process,
definition ID, definition version, and definition digest. It rejects
malformed input/outcome/action schemas, cross-definition scope, sequence,
duplicate-outcome identity, duplicate action identity, duplicate semantic
action effect keys, and oversized outcome or action batches before an adapter
sees the request, and is parser/validation fuzzed. A test-only atomic-store
drill injects a failure before inbox staging,
after inbox staging, after outcome append staging, and after action enqueue
staging. At each point it proves no input, outcome, or action becomes visible;
recovery commits exactly once, duplicate inbox delivery is idempotent, stale
sequence conflicts, and a bounded outcome page replays the committed state.
No durable implementation or projection write exists yet, so this item remains
incomplete.

### P1.3 Inbox/outbox and worker semantics

- [ ] Implement tenant-scoped inbox dedup by immutable source event ID and
  outbox records with schema, action ID, payload digest, attempts and ack.
- Why: brokers provide at-least-once delivery, not business exactly-once.
- How: acknowledge an input only after commit; publish at least once; every
  consumer idempotently handles its action ID.
- Evidence: duplicate/reordered/redelivered messages, crash-before-ack and
  crash-after-publish drills.

Current partial evidence: the atomic commit contract carries the immutable
optional inbox input and outgoing actions together with outcomes, so an adapter
has an explicit no-split-write boundary to implement. Inbox/outbox storage,
acknowledgement, leases, redelivery, and crash drills remain incomplete. The
versioned `OutboxRecordV1` now makes action, delivery attempt, and explicit
pending/acknowledged state part of the reusable port contract, with a bounded
attempt validator and DTO fuzz coverage. Monotonic transition helpers make
acknowledgement idempotent and reject redelivery after acknowledgement or
attempt exhaustion. Lease/fencing and durable dispatch semantics remain
adapter-owned and incomplete. The backend-neutral `OutboxStore` port now
exposes bounded, scope-pinned claim and exact-record acknowledgement
operations; implementations must supply leases/fencing and durable redelivery
behavior. `OutboxLeaseTokenV1` and `OutboxLeaseV1` now provide an opaque
fencing token, owner, and expiry for stale-worker protection; adapters still
own token allocation and expiry enforcement. `OutboxLeaseV1::validate_for_claim`
also rejects a lease whose action scope differs from the claim scope, while
`validate_at` rejects acknowledgement after the logical expiry boundary.
Claim requests now carry the authenticated owner principal, and lease
validation rejects owner substitution.
`validate_for_claim_at` combines request-bound scope/owner checks with expiry
validation for one fail-closed adapter call.
The `OutboxStore::acknowledge` port now requires a typed logical `now` value,
making expiry enforcement an explicit adapter contract rather than an optional
caller convention. `acknowledge_at` provides the pure lease-safe transition
that adapters can apply before persisting the acknowledgement. `renew_at` and
the `OutboxStore::renew` port require an unexpired lease and strictly later
logical expiry, preventing stale or non-extending renewals. Renewal is also
rejected after acknowledgement, preventing lease resurrection. `Inbox::accept`
now returns an `InboxAcceptanceReceiptV1` carrying the exact input identity and
duplicate flag, so redelivery can be a durable no-op without rerunning a
   decision. `InboxAcceptanceReceiptV1::validate_for` now rejects an adapter
   receipt bound to another input identity. Storage, acknowledgement ordering,
   and crash recovery remain adapter-owned.

### P1.4 StateChronicle contract

- [ ] Publish types and a guide for canonical command submission and committed
  event correlation.
- Why: Penelope must coordinate process truth without inventing ledger facts.
- How: implement `penelope-statechronicle` without a local path dependency;
  publish a versioned adapter contract. In the Penelope append transaction,
  persist the planned command action and outbox record. Submit with that exact
  action ID as StateChronicle idempotency ID through a verified durable path.
  Consume transactional-outbox events in a durable Penelope inbox and validate
  tenant, command/action ID, expected operation, resource scope and committed
  result before advancing. Reconcile pending actions by ID after restart.
- Evidence: E2E fake-ledger cases for lost response, duplicate command, delayed
  event, rejection, restart, compensation, wrong-tenant event, mismatched
  event/action ID, unverified event and settlement-unknown state.

Current partial evidence: `penelope-statechronicle` provides a typed command
expectation and a verifier that accepts a canonical event only when tenant,
action ID, operation, exact bounded duplicate-free resource scope, and expected
redacted committed-result digest all match. The verified result retains the
pinned Penelope process/definition scope, preventing a caller from losing that
authorization context after correlation. It rejects malformed expected or
received scopes before correlation, has unit tests and the
`fuzz_statechronicle_correlation` target. It deliberately does not claim
that a transport response proves a commit and does not implement a client.
`CanonicalCommandExpectationV1::from_command` now derives the action,
operation, and resource scope from a validated command, reducing field-copying
confusion at the consumer composition root; its equality is unit tested.
`AtomicProcessCommitV1` now accepts a canonical source-event key only beside a
canonical inbox input, requiring the store to deduplicate that key in the same
transaction as the input, outcomes, and actions. Its fault-injection drill
proves source redelivery under a different inbox ID cannot append a second
outcome after recovery. `bind_verified_event_to_commit` provides the
user-facing safe path from verified event to atomic source-bound commit; it
rejects input kind, scope, digest, and outcome-scope substitution, with typed
tests for source pinning and digest substitution. Response-loss recovery and a
durable adapter remain incomplete.

### P1.5 External executor ambiguity

- [ ] Specify executor request/result, effect key, external-reference, timeout
  and cancellation contracts.
- Why: a network timeout after remote success must not trigger a blind retry.
- How: reconcile known external references; route non-idempotent unknown results
  to review rather than assuming failure.
- Evidence: mock tests for crash, timeout-after-success, duplicate request and
  inconsistent remote status.

Current partial evidence: `CanonicalReconciliationV1` makes three mutually
exclusive typed states explicit: committed with immutable event evidence,
authoritatively not committed, and unknown. It validates that the evidence
belongs to the requested action ID and complete pinned process-definition
scope; committed evidence must also carry a valid same-tenant event. The port
contract prohibits treating unknown as retry permission. It is parser/validation
fuzzed. `ExternalEffectExecutor` additionally defines a typed remote reference,
effect-key, optional deadline, reconciliation, and best-effort cancellation
contract; its unknown state remains explicitly non-retryable. There is no
executor policy, consistency-window implementation, or fault-injected adapter
drill yet, so this item remains incomplete. `EffectDispatchRequestV1::validate_at`
now provides an inclusive logical deadline check, returning typed
`DeadlineExceeded` instead of leaving expiry behavior to adapter conventions.

### P1.6 Manual review lifecycle

- [ ] Add create, claim, evidence, decision, optional dual-control, expiry and
  audit outcomes.
- Why: financial and high-value game workflows need accountable intervention.
- How: record who/why/when/which sequence; require authorized immutable input.
- Evidence: concurrent operator, expired review, authorization and audit tests.

Current partial evidence: `ManualReviewClaimV1` and `ManualReviewDecisionV1`
are versioned typed port DTOs. They require the exact pinned process-definition
scope, a validated `PrincipalId`, typed resolution/control enums, immutable
review ID, and redacted evidence digest; both validate against the durable
review's full scope. The decision may require a distinct deciding principal
from the claimant, and the validator fails closed when that dual-control rule
is violated. Claim and decision validation also rejects a malformed review
record before using its scope. Reviews can carry an immutable inclusive logical
expiry; claims or decisions after it fail closed. The review port records open,
claim, and decision separately so no
operator directly mutates a projection. Their authorized resolution is now an
explicit replayable engine input rather than a direct projection mutation. They
have parser and engine-transition fuzz coverage through the public review
lifecycle and linear engine targets. Authorization policy, persistence,
delivery as an inbox input, conflict behavior, and adversarial operator drills
remain incomplete.

### P1.7 Security and supply chain baseline

- [ ] Add `SECURITY.md`, disclosure/support policy, licenses, dependency policy,
  Dependabot/Renovate, `cargo audit`, `cargo deny`, secret scanning and locked
  CI builds.
- Why: workflow code can amplify a dependency or authorization flaw into
  repeated high-value actions.
- How: define MSRV, pin or govern CI action updates, and retain audit artifacts.
- Evidence: CI gates and documented vulnerability response rehearsal.

Current partial evidence: `SECURITY.md` defines supported pre-1.0 scope,
private reporting, response expectations, and saga-specific safety boundaries.
Complete `LICENSE-MIT` and `LICENSE-APACHE` texts match the declared manifest
license. Locked CI builds exist, and previously unused `chrono`, `tokio`, and
`trait-variant` workspace declarations have been removed. `deny.toml` permits
only the reviewed MIT, Apache-2.0, NCSA, Unicode-3.0, and Unlicense dependency
licenses, rejects unknown registries/git sources, and CI runs both `cargo audit`
and `cargo deny` across advisories, bans, licenses, and sources. Secret
scanning is fail closed locally over reachable history and runs in CI over the
complete checkout history on every push and pull request. Action pin governance,
MSRV policy, and a response rehearsal remain incomplete.

## P2 — adapters, operations and user experience

### P2.1 Reference relational adapter

- [ ] Add `penelope-postgres` with migrations and contract tests; SQLite may be
  local/test-only if documented concurrency limits are acceptable.
- Why: users need a proven durable pattern while the core remains storage-free.
- How: one transaction for append/project/inbox/outbox; DB tenant/sequence
  constraints; serialization-conflict-only retries; TLS in deployments.
- Evidence: live DB suite, migration rehearsal, concurrent writers and
  kill/restart recovery drills.

### P2.2 Timer/outbox worker references

- [ ] Provide safe due-timer claiming and outbox dispatch with leases/fencing,
  bounded batches, backoff, ownership transfer and dead-letter escalation.
- Why: worker crashes and lease races commonly duplicate external effects.
- Evidence: contention, lease expiry, double fire, clock skew and partition
  drills with deterministic assertions.

### P2.3 Operations and observability

- [ ] Define structured logs, traces and bounded-cardinality metrics for
  lifecycle, replay, queue depth, timer lag, retry, dedup, outbox lag,
  compensation and review.
- [ ] Write deployment, backup/restore, incident recovery, retention/redaction,
  capacity and SLO guides.
- Why: operators need evidence of liveness and recovery without leaking data.
- Evidence: dashboard/alerts, restore rehearsal and stated RPO/RTO.

### P2.4 Complete documentation

- [ ] Add rustdoc for every public API, ADRs, threat model, compatibility and
  upgrade policies, plus compiled examples for settlement, venue lifecycle,
  retries, compensation and review.
- Why: orchestration contracts are easy to misuse if only implied.
- Evidence: strict rustdoc and compiled example CI gate.

## P3 — proof through testing, failure and load

### P3.1 Automated test and fuzz matrix

- [ ] Add unit, integration, model-based, property and fuzz testing for every
  transition, parser and adapter contract.
- Why: failures arise from combinations of retries, reordering and restarts.
- Evidence: scheduled extended fuzzing, retained minimized regressions and
  deterministic CI coverage.

Current partial evidence: `.github/workflows/ci.yml` runs stable format, tests,
Clippy, strict docs, benchmark build, and both reference examples, plus every
fuzz target for 1,000 inputs under nightly. Extended scheduled fuzzing and
retained/minimized crash-regression policy remain incomplete.

On the current revision (including the bounded graph executor and its execution
fuzz paths), all nine registered fuzz targets were run concurrently for 3,601
seconds each. They completed with exit code 0, normal libFuzzer summaries,
3,836,870,761 total executions, and no sanitizer, undefined-behavior,
runtime-error, or crash markers. This is pure library-boundary evidence; it
does not replace durable-adapter or deployment chaos testing.

### P3.2 Fault injection and chaos

- [ ] Inject failures around every durable boundary, lease, timer dispatch,
  command/event delivery, restart, failover, disk pressure and clock skew.
- Why: reliability claims require survival of partial failure.
- Evidence: repeated drills prove no lost outcomes, no duplicate canonical
  command, replay validity and explicit ambiguity escalation.

Current partial evidence: the linear engine has a generated legal-transition
property that replays from the durable start record and after every individual
recorded action-result event, across success, retryable failure, terminal
failure, unknown outcome, compensation, escalation, and completion paths. Each
restart must equal the live projection and pending action decision. This is
pure-engine crash-boundary evidence only; it does not exercise durable writes,
worker crashes, network partitions, or external canonical effects. A focused
retry-timer drill additionally proves early/duplicate timer fires fail closed
and replay preserves the due-time transition. Durable timer adapters and chaos
remain incomplete. `scripts/run_pure_chaos_drill.sh` repeats the executor's
restart/timer properties and every current public-boundary fuzz target; a
scheduled workflow runs the bounded extended drill weekly. This remains pure-library evidence, not a
durable deployment drill.

### P3.3 Reproducible performance evidence

- [ ] Benchmark pure decisions/replay, durable append/project, inbox/outbox,
  timer claiming and E2E StateChronicle coordination.
- Why: throughput primarily depends on storage, batching, contention, workers
  and cache locality, not only pure transition speed.
- How: report hardware, topology, Rust/DB versions, payloads, cardinality,
  contention, p50/p95/p99, throughput and errors; separate cold/warm and
  single/multi-thread results.
- Evidence: versioned artifacts and regression thresholds; no unqualified
  “millions of operations” claim.

Current partial evidence: the new `graph_saga` release benchmark measures a
two-step pure graph start/result/result operation at approximately 985 ns/op
(1.01M ops/s) on the local host. This excludes serialization, storage,
network, contention, and adapter coordination; p95/p99 and regression
thresholds remain to be established. The parallel companion benchmark measured
3,200,000 isolated operations across 32 workers at approximately 12.25M ops/s
on the same host; this is not shared-process or durable-store throughput. Graph
replay duplicate-input detection now uses a bounded hash set, avoiding
quadratic scans under the 4,096-event replay cap.

Current partial evidence: `cargo bench -p penelope-executor --bench
linear_saga --locked` measures a fixed 100,000-operation pure
start/correlated-result/complete loop without external I/O. `cargo bench -p
penelope-executor --bench parallel_linear_saga --locked` runs the same pure
transition across one isolated in-memory worker per available logical CPU,
with 100,000 operations per worker. Neither benchmark shares process state,
uses a store, coordinates workers, or crosses a network boundary. They are
therefore deliberately not E2E, durable-store, shared-state, or market-engine
throughput claims. Each result must be reported with the exact command, source
revision, worker count, and executing hardware. The additional
`replay_saga` benchmark replays a fixed two-event recovery log one million
times; it measured 405 ns/replay in the current release build. This is still
pure recovery cost and does not establish durable or network throughput.

### P3.4 Financial/economic security review

- [ ] Threat-model tenant crossover, replay, forged events, command confusion,
  privilege escalation, secrets, resource exhaustion, compromised workers and
  timing.
- [ ] Require independent review for workflows causing capital movement or
  trading actions.
- Evidence: reviewed model, test matrix, incident tabletops, kill-switch and
  remediation tracking.

## First production release gate

- [ ] All P0/P1 code, docs and automated evidence complete.
- [ ] One relational adapter passes contract, concurrent-writer and recovery
  suites.
- [ ] StateChronicle integration proves duplicate, delayed, rejected and
  ambiguous-command handling end to end.
- [ ] CI enforces format, tests, Clippy, strict docs, audit/deny, property and
  bounded fuzz tests with locked dependencies.
- [ ] Backup/restore, broker redelivery, worker crash, timer race and DB
  failover drills repeatedly pass.
- [ ] Security review, compatibility policy, release notes and license texts
  are complete.
- [ ] Any performance statement has a reproducible workload and hardware report.
