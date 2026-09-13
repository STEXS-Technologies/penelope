# Penelope implementation TODO

## Current state

This repository is a documentation-only Rust scaffold. Nothing below is
implemented unless checked off with linked tests and reproducible evidence. Do
not call Penelope production-ready before every P0 and P1 item is complete.

## Priority order

| Priority | Outcome | Why it precedes later work |
| --- | --- | --- |
| P0 | Deterministic domain engine | Without replay-safe behavior, adapters only persist bugs. |
| P1 | Durable contracts and integration safety | A workflow must survive crashes, retries and duplicate delivery. |
| P2 | Reference adapters and operational controls | Users need proven persistence and recovery patterns. |
| P3 | Performance, chaos and ecosystem maturity | Scale claims are meaningful only after safety is established. |

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

### P0.2 Immutable versioned definitions

- [ ] Implement `ProcessDefinition`, `DefinitionVersion`, `StepDefinition`,
  transitions, correlation rules, retry/timeout policy and compensation policy.
- Why: running instances must retain their original semantics after deployment.
- How: validate at registration; pin definition version and content digest at
  start; require explicit terminal states; reject ambiguous ordering and
  unbounded cycles unless a bounded-loop construct is specified.
- Evidence: tests for invalid graphs, version pinning, digest mismatch, graph
  bounds and rejected incompatible migration.

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

### P0.4 Pure transition and replay function

- [ ] Implement `decide(definition, projection, input) -> Decision` and
  `apply(projection, outcome) -> Result<Projection, DomainError>`.
- Why: pure deterministic logic enables audit, recovery and exhaustive tests.
- How: pass time/entropy explicitly; sort collections before decisions; prohibit
  I/O, globals and wall-clock access in the core; issue stable action IDs.
- Evidence: repeated-decision determinism tests, no-panic fuzz/property tests,
  and decision/apply consistency tests.

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

### P0.7 Compensation and operator escalation

- [ ] Build a persisted LIFO compensation plan for succeeded compensable steps.
- [ ] Prevent a compensation effect running twice; route irrecoverable or
  ambiguous outcomes to an immutable manual-review action.
- Why: distributed systems cannot use one ACID transaction across services.
- How: classify irreversible steps; record each compensation result; manual
  resolution is an authorized input, never a direct projection mutation.
- Evidence: partial settlement, duplicate compensation, compensation failure,
  operator resolution and irreversible-step scenarios.

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

### P1.2 Atomic append/project/inbox/outbox boundary

- [ ] Define compare-and-append that atomically validates expected sequence,
  appends outcomes, updates projection, stores outgoing actions and deduplicates
  the input event.
- Why: split writes create crash windows that lose actions or repeat effects.
- How: optimistic concurrency per instance; conflict means no partial writes;
  outbox and inbox state share the durability transaction.
- Evidence: failpoints before/after every write and recovery convergence tests.

### P1.3 Inbox/outbox and worker semantics

- [ ] Implement tenant-scoped inbox dedup by immutable source event ID and
  outbox records with schema, action ID, payload digest, attempts and ack.
- Why: brokers provide at-least-once delivery, not business exactly-once.
- How: acknowledge an input only after commit; publish at least once; every
  consumer idempotently handles its action ID.
- Evidence: duplicate/reordered/redelivered messages, crash-before-ack and
  crash-after-publish drills.

### P1.4 StateChronicle contract

- [ ] Publish types and a guide for canonical command submission and committed
  event correlation.
- Why: Penelope must coordinate process truth without inventing ledger facts.
- How: use a durable command ID per canonical mutation; submit through a
  verified StateChronicle durable path; advance only after matching committed
  evidence appears in the inbox.
- Evidence: E2E fake-ledger cases for lost response, duplicate command, delayed
  event, rejection, restart and compensation.

### P1.5 External executor ambiguity

- [ ] Specify executor request/result, effect key, external-reference, timeout
  and cancellation contracts.
- Why: a network timeout after remote success must not trigger a blind retry.
- How: reconcile known external references; route non-idempotent unknown results
  to review rather than assuming failure.
- Evidence: mock tests for crash, timeout-after-success, duplicate request and
  inconsistent remote status.

### P1.6 Manual review lifecycle

- [ ] Add create, claim, evidence, decision, optional dual-control, expiry and
  audit outcomes.
- Why: financial and high-value game workflows need accountable intervention.
- How: record who/why/when/which sequence; require authorized immutable input.
- Evidence: concurrent operator, expired review, authorization and audit tests.

### P1.7 Security and supply chain baseline

- [ ] Add `SECURITY.md`, disclosure/support policy, licenses, dependency policy,
  Dependabot/Renovate, `cargo audit`, `cargo deny`, secret scanning and locked
  CI builds.
- Why: workflow code can amplify a dependency or authorization flaw into
  repeated high-value actions.
- How: define MSRV, pin or govern CI action updates, and retain audit artifacts.
- Evidence: CI gates and documented vulnerability response rehearsal.

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

### P3.2 Fault injection and chaos

- [ ] Inject failures around every durable boundary, lease, timer dispatch,
  command/event delivery, restart, failover, disk pressure and clock skew.
- Why: reliability claims require survival of partial failure.
- Evidence: repeated drills prove no lost outcomes, no duplicate canonical
  command, replay validity and explicit ambiguity escalation.

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
