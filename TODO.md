# Penelope implementation TODO

## Current state

This repository contains an early pure-engine slice: versioned typed DTOs,
validated newtype identities, ports, a reference linear state machine, unit and
property tests, and parser/DTO/engine fuzz targets. It has no durable outcome
store, production adapter, or production-readiness evidence. Nothing below is
complete unless checked off with linked tests and reproducible evidence. Do not
call Penelope production-ready before every P0 and P1 item is complete.

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
does not yet enforce all public DTO schema fixtures, raw-string port
signatures, or fuzz-target coverage, so this item remains incomplete.

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
payload/instance/timer limits, golden serialization fixtures, and
compile-time non-substitutability tests remain incomplete.

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
tested and the changed serde definition boundary is fuzzed. Graph validation, registration,
compatibility/migration policy, non-linear transitions, full digest
calculation, and deployment-time definition storage remain incomplete.

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
engine does not yet emit the complete lifecycle outcome log or apply these
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
through the serialized linear definition boundary. There is no jitter,
persisted timer record, engine-issued timer decision, or timer race handling.
The ports expose a due-time timer schedule boundary, so this item remains
incomplete.

Current partial P0.5 evidence: the projection retains all action IDs issued by
one process. A result must correlate to the active ID, and any next/retry ID
already issued by that process is rejected. This does not replace durable
inbox/outbox deduplication or an effect-key contract. Each emitted action now
also carries the pinned definition ID/version/digest alongside tenant, process,
step, attempt, kind and action ID, preventing an adapter from losing the
definition deployment context. P0.5 remains
incomplete.

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
persisted compensation outcome schema or manual-review workflow yet, so this
item remains incomplete.

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

Current partial evidence: backend-neutral `Clock`, `ActionIdSource`,
`ProcessAuthorizer`, and due-time `TimerScheduler` ports now use typed logical
time, process scope, principal, action, and authorization operation values.
Their public DTO parser boundary is fuzzed. Definition/process-store, inbox,
outbox, executor and review interfaces exist in smaller forms, but there are
no fake adapters, shared contract suite, authorization policy, or cancellation
semantics evidence yet, so this item remains incomplete.

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
cross-definition, scope, and sequence mismatches before an adapter sees the
request, and is parser/validation fuzzed. No durable implementation,
optimistic conflict behavior, projection write, or failpoint recovery test
exists yet, so this item remains incomplete.

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
acknowledgement, leases, redelivery, and crash drills remain incomplete.

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
action ID, operation, and exact bounded, duplicate-free resource scope all
match. It rejects malformed expected or received scopes before correlation, has
unit tests and the `fuzz_statechronicle_correlation` target. It deliberately does not claim
that a transport response proves a commit, does not implement a client, and
does not yet cover source freshness/deduplication or response-loss recovery.

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
belongs to the requested action ID, and the port contract prohibits treating
unknown as retry permission. It is parser/validation fuzzed. There is no
executor policy, remote reference contract, consistency-window implementation,
timeout/cancellation model, or fault-injected adapter drill yet, so this item
remains incomplete.

### P1.6 Manual review lifecycle

- [ ] Add create, claim, evidence, decision, optional dual-control, expiry and
  audit outcomes.
- Why: financial and high-value game workflows need accountable intervention.
- How: record who/why/when/which sequence; require authorized immutable input.
- Evidence: concurrent operator, expired review, authorization and audit tests.

Current partial evidence: `ManualReviewClaimV1` and `ManualReviewDecisionV1`
are versioned typed port DTOs. They require a validated `PrincipalId`, typed
resolution enum, immutable review ID, and redacted evidence digest; the review
port records open, claim, and decision separately so no operator directly
mutates a projection. They have parser fuzz coverage through the public review
lifecycle target. Authorization policy, expiry, dual control, persistence,
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
license. Locked CI builds exist. Dependency policy, automated audit/deny,
secret scanning, action pin governance, MSRV policy, and a response rehearsal
remain incomplete.

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
worker crashes, network partitions, timers, or external canonical effects, so
this item remains incomplete.

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

Current partial evidence: `cargo bench -p penelope-executor --bench
linear_saga --locked` measures a fixed 100,000-operation pure
start/correlated-result/complete loop without external I/O. It is deliberately
not an E2E, durable-store, or multithread throughput claim. Its measured output
must always be reported with the executing hardware and command result.

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
