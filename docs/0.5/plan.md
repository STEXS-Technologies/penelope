# Penelope 0.5.0 — Unified Deterministic State and Execution Architecture

**Status:** Implementation proposal, not an implemented or released capability
**Baseline audited:** `STEXS-Technologies/penelope`, released `v0.4.0`, `main` at `42ec187c9adfc7403b06215f405f2575f53c35c2` (2026-11-01 commit; `v0.4.0` published 2026-11-02)
**Audit date:** 2026-11-14
**Target:** A coherent Penelope 0.5.0, not a fourth unrelated engine
**Compatibility policy:** Preserve existing 0.4.0 durable histories and guarantees; add a new independently versioned machine protocol; provide deliberate source migration paths
**Primary language:** Rust 2024, MSRV 1.85.0 unless an explicit, separately approved decision changes it

> **Central design decision:** Penelope becomes one framework for deterministic state evolution and reliable execution. A typed **machine** is its general application programming model. Sagas are long-lived, effect-coordinating machines; recoverable tasks are execution-lifecycle machines; timelines are an optional versioned history/correction mode for compatible deterministic machines. All four share one conceptual transition pipeline and common primitives, while retaining their *different* safety and durability semantics. The refactor must consolidate duplicate internals and unify the public model, not merely rename modules or add a `penelope-machine` crate alongside unrelated code.

---

## 1. Executive decisions

1. **One public framework.** The canonical consumer API lives under the `penelope` facade, with discoverable `machine`, `saga`, `task`, `timeline`, `storage`, and integration namespaces. Internal Cargo crates remain modular deployment/build boundaries; the user should not need to understand that internal layout to use the framework.
2. **One fundamental operation.** `state + admitted event + pinned model + deterministic context -> accepted/rejected transition + tentative effect intents`. A prepared transition does **not** mutate the live state or execute external effects. The applicable publication boundary makes it authoritative.
3. **First-class state machines.** Users can model arbitrary typed application states/events, guarded transitions, state-local data, hierarchical event handling, entry/exit order, explicit refusal, and structured transition provenance. There is no mandatory database, task, saga, timeline, async runtime, macro, or scheduler.
4. **Integrate existing engines; do not erase their contracts.** Make existing linear/graph sagas and task projections usable through the common machine-facing APIs. Share validation, staging, identity/provenance and commit plumbing where semantics are truly identical. Do not merge effect recovery into generic state transitions, nor rewrite v1 histories into a new format.
5. **Timeline remains a mode, not the global definition of state.** Reuse 0.4.0 `RewindableComputation`, scratch replay, `LiveTimeline`, finality, and exact-head fencing for eligible machines; normal durable machines must not pay for snapshot cloning, branches, or speculative rewind.
6. **Durability is opt-in and explicit.** Provide a machine-specific atomic transition journal/CAS/outbox contract and a qualified SQLite implementation. Do not misuse saga `AtomicProcessCommit`, which binds saga-specific `ProcessInput`/`ProcessOutcome`/`ProcessAction` records.
7. **Effect ambiguity is never fabricated away.** A committed state transition may authorize an effect intent; a transport ACK, timeout, or crash cannot establish the effect's success. Existing saga/task reconciliation and generation fencing remain in force.
8. **No default feature explosion.** The pure transition path remains `no_std`, no implicit heap allocation, and independent of storage/hosting. Persistence, timeline, runtime, and ergonomic macros (if ultimately justified) are opt-in.
9. **A real refactor has removal criteria.** The release is not complete while consumers must choose between competing concepts of “state”, “transition”, “intent”, or “history” without an explicit, documented semantic reason. Legacy entry points may remain as compatibility shims; there must be a single documented canonical path.
10. **Qualification over feature counting.** Existing 0.4.0 reproducible benchmarks, conformance checks, property tests, crash/fence tests, and wire fixtures remain release gates. A reduction in source lines is valuable only if all invariants survive.

### Non-goals for 0.5.0

- A general-purpose distributed scheduler, broker, database, actor system, or durable-execution SaaS.
- A claim that Rust can statically prove every runtime event legal. State-local data is modeled in enums; legal event/state combinations involving runtime inputs are validated by the transition function.
- Transparent rollback of effects already committed to an external system, or automatic compensation of arbitrary application mutations.
- A mandatory proc-macro DSL, generated SQL per user-defined machine, or magic interception of arbitrary direct Rust field mutation.
- Durable persistence of the entire 0.4.0 timeline state via a feature that does not yet exist. Timeline recovery beyond its current documented contract requires a distinct, tested adapter profile.
- Parallel/orthogonal hierarchical regions, SCXML interpreter compatibility, or distributed transactions across StateChronicle and independent stores. These may be separate later features.

---

## 2. Verified real-world 0.4.0 baseline

### 2.1 Repository and released surface

The inspected repository declares **ten library workspace packages** (`penelope-core`, `penelope-domain`, `penelope-intent`, `penelope-executor`, `penelope-ports`, `penelope-statechronicle`, `penelope-timeline`, `penelope-sqlite`, `penelope-process-runtime`, and `penelope`) plus the fuzz workspace member. `v0.4.0` is a published GitHub release; the current inspected `main` has no implementation of a public arbitrary application-machine API. The release and docs describe the 0.4.0 timeline as an independent addition. This proposal is grounded in source inspection, **not** a fresh local compile/benchmark of the repository.

| Existing subsystem | Verified implementation | What it already contributes | Boundary that must not be misrepresented |
| --- | --- | --- | --- |
| Linear saga | `penelope-executor/src/engine.rs` | Version-pinned definitions, steps, typed result decisions, retries, compensation, replay, status | Specializes in executable steps, not arbitrary application-state events |
| Graph saga | `penelope-executor/src/engine.rs` + `graph.rs` | `ProcessGraphDefinition`, `GraphTransition`, `start_graph`, `apply_graph_result`, ordered replay | Edges are keyed by action result category; no general hierarchical machine API |
| Recoverable task | `penelope-core/src/task/{contracts,projection,control,runtime}.rs` | `TaskProjection`, `TaskStatus`, bounded history, explicit recovery permission, generation fencing, checkpoints, pure control | Governs task execution lifecycle, not application business-state transitions |
| Timeline | `penelope-core/src/timeline/*`, `penelope-timeline/src/*` | Immutable inputs, state anchors, scratch correction, exact-head publish, tentative effects, separate effect finality | Generic application computation remains consumer-owned; no persistent timeline recovery adapter promised |
| Process ports | `penelope-ports/src/implementation.rs` | `AtomicProcessCommit`, `ProcessStore`, inbox/outbox, timer, authorization, evidence receipts | DTOs/commit semantics are specific to process outcomes and actions |
| SQLite | `penelope-sqlite/src/{process,tasks,schema,worker}.rs` | WAL/FULL single-host adapter, process commits, task journal, bounded worker queue, quotas | Does not presently expose a machine journal; digests are not authenticity proofs |
| Process runtime | `penelope-process-runtime/src/*` | Protected Linux workers, execution generation enforcement, bounded IPC | This is an optional runtime, not a default machine scheduler |
| StateChronicle bridge | `penelope-statechronicle/src/*` | Correlates verified canonical events to exactly scoped external facts | StateChronicle remains canonical for its domain; Penelope does not inherit its authority |
| Public facade | `penelope/src/lib.rs`, `penelope/Cargo.toml` | Re-exports engine/graph/task/ports; timeline under optional feature | Public concepts are exposed by subsystem rather than one cohesive usage pattern |

### 2.2 Specific source facts that constrain the refactor

- `GraphTransition` has `from_step`, `on: GraphTransitionOutcome`, and `to_step`; the outcome has `Succeeded`, `RetryableFailure`, `TerminalFailure`. `ProcessGraphDefinition::validate()` checks bounds, duplicate steps/edges, invalid references and reachability. `graph.rs` also has active-action identity checking, unknown-effect escalation, visit limits and ordered replay.
- `TaskProjection::apply()` builds a copied candidate, validates a `TaskRecord` and only assigns the candidate on success. The documented task states are `Empty`, `Pending`, `Running`, `Faulted`, `Reconciling`, `Reconciled`, `Ready`, `Completed`, and `Exhausted`.
- `TaskControl` commits at most one history record per advance and poisons after ambiguous journal/runtime outcomes until reconstruction. Any attempt to “unify” it must retain this behavior exactly.
- The timeline's `prepare_live_step()` computes on isolated scratch, validates, and returns an opaque `ReadyStep`; `LiveTimeline` publishes state/head/effects as one owner. `ReplaySession` aborts on failure and has a separate promotion step. `EligibleEffect` and `EffectAdmissionReceipt` distinguish eligibility, durable admission, and actual effect execution.
- The timeline metadata format is independently tagged `penelope.timeline.v1`. Task records and saga v1 records have independent existing compatibility constraints. Machine records require their **own** namespace and version, rather than retroactive edits to these formats.
- The facade's only optional feature is currently `timeline`; `penelope-core` and `penelope-timeline` use default-empty feature sets. CI exercises stable and Rust 1.85.0, a RISC-V bare-metal target, Clippy, rustdoc, fuzz/property tests and conformance tests.
- `scripts/check-layer-boundaries.sh` hardcodes the current permitted dependency graph. Adding or moving modules requires changing that rule deliberately rather than weakening it.

### 2.3 Actual fragmentation and debt to address

**A. Two separate transition implementations plus a third computational convention.** Linear/graph saga decisions and task `TaskProjection` have different transition frontends; timeline uses `RewindableComputation::step`. That is justified by protocol history but not by the desired 0.5.0 consumer mental model.

**B. Repeated infrastructure concerns.** Scope pinning, outcome/event identity, monotonic sequences, resource bounds, deterministic replay and stale-write fencing appear independently. Consolidate *primitives* and shared verification helpers, but retain distinct wire-level identifiers where semantics differ.

**C. Effect terminology crosses several stages.** Saga `ProcessAction`, task effect evidence, and timeline staged intents are not interchangeable. Introduce one explicit public lifecycle vocabulary—`proposed -> committed/admitted -> dispatched -> verified/reconciled`—and narrow adapters that preserve the exact original protocol's meaning.

**D. Persistence is not a generic free-standing model.** Process and task persistence exist, but arbitrary application-machine commits do not. A machine needs its own atomic, independently encoded state/event/effect records before “durable machines” can be advertised.

**E. Debuggability is incidental.** Replay and typed records exist, but there is no uniform application-facing answer to “which event, state, guard, transition, revision, and effect caused this result?”. This becomes a documented first-class transition inspection contract.

**F. Consumer onboarding requires cross-crate knowledge.** One user-facing contract and focused import paths should replace having to understand `engine`, `graph`, `task`, `timeline`, `ports` and `statechronicle` just to model an order.

### 2.4 What should *not* be refactored away

Existing action/result coordinates, task generation and scope, timer logical time versus task ticks, timeline branch/revision/authority frontiers, compensation ordering, SQLite WAL/FULL constraints, and StateChronicle correlation are **semantically distinct**. A shared trait must not erase their types or provide automatic conversions that weaken validation. “Unified” means shared principles and a compatible orchestration surface, not one giant `Event` enum or single universal database table.

---

## 3. Coherent 0.5.0 system model

### 3.1 One runtime-neutral transition pipeline

```text
Authenticated/admitted external request or local event
                  |
                  v
         [Pinned machine definition]
                  |
                  v
      [Current state + versioned scope]
                  |
        deterministic evaluation
                  v
    [Transition candidate / explicit refusal]
          |                     |
  accepted candidate       rejected/unhandled
          |                     |
   validated invariants       no state mutation,
          |                    no effect dispatch
          v
 [Publication / durable CAS boundary]
          |
    committed state + event provenance
          |
          +------> [committed effect intents]
                             |
                        admission / dispatch
                             |
                  verified outcome or reconciliation
                             |
                      follow-up typed event
```

The conceptual layers are:

1. **Definition:** stable machine identity, schema/code compatibility and declared state/event/transition semantics.
2. **Decision:** pure, deterministic evaluation against an immutable current state; errors and rejected/unhandled events do not publish.
3. **Publication:** in-memory owner assignment or atomic durable transition CAS, with stable provenance and state/event/effect binding.
4. **Execution:** optional tasks, sagas, timers and outbound effects; never automatically run inside a pure transition.
5. **History:** ordered machine events/revisions; optional snapshots; optional bounded timeline correction only when safe.
6. **Observability:** structured transition and effect lifecycle evidence derived from committed records, with rejected attempts explicitly separated from state history.

### 3.2 How existing features fit

| Penelope concept | Unified interpretation | Existing behavior that must survive |
| --- | --- | --- |
| `machine` | General typed application state + event reducer/transition handler | New capability; pure path usable alone |
| `saga` | Built-in machine that plans/coordinates externally executed steps | v1 process records, retries, compensation, action correlation, manual review |
| `task` | Built-in machine that manages recoverable execution and checkpoints | task v1 records, generation fencing, crash ambiguity, quotas, no blind restarts |
| `timeline` | Optional history/revision/correction **mode** for deterministic model state | 0.4.0 bounded replay, branch lineage, scratch isolation, finality and CAS |
| `storage` | Durable admission, journaling, snapshots and outbox ports | Independent store contracts and authoritative receipts |
| `runtime` | Pluggable worker and timer integrations | Protected process runtime and no mandatory host executor |
| `integration::statechronicle` | External canonical commitment verification | No conversion of Penelope's local state into external canonical truth |

**Key rule:** A saga is not literally a generic order enum, and a task is not simply a saga step. They are *specialized state machines with additional contracts*. A common decision API can wrap their state transitions without claiming that a generic reducer has already implemented their domain-specific correctness.

### 3.3 Single public experience; modular implementation

The user should normally write:

```rust
use penelope::machine::{Machine, Transition, MemoryMachine};
use penelope::saga::{/* orchestration API */};
use penelope::task::{/* recovery API */};
use penelope::timeline::{/* optional correction API */};
```

The above illustrates **proposed 0.5.0 namespace design**, not current compiling imports. Experienced users may depend directly on internal crates when necessary; the primary docs and examples should use the top-level `penelope` API first.

Keep these dependency boundaries:

```text
penelope-core       (no_std, no alloc required; shared decision primitives)
       | \
       |  +-- penelope-timeline (isolated replay; existing implementation)
       +----- penelope-domain   (legacy saga DTOs/codec; preserve wire)
       +----- penelope-intent   (legacy input validation)
       +----- penelope-ports    (hosted contracts incl. new machine journal)
                    |
             penelope-executor  (saga/task/machine composition)
                    |
        penelope-sqlite / penelope-process-runtime
                    |
                 penelope       (one canonical consumer facade)
```

The diagram depicts responsibility, not an exact Cargo dependency graph; preserve actual DAG constraints and avoid back-edges. **Do not create a new `penelope-machine` package merely because it is a named concept.** Put the portable kernel in `penelope-core::machine`, hosted composition in `penelope-executor::machine`, storage ports in `penelope-ports::machine`, and SQLite implementation in `penelope-sqlite::machine`. Only split another published crate if proven build-size, feature isolation, or stability requirements demand it.

---

## 4. Shared machine primitives — core implementation

### 4.1 Core contract (proposed API sketch)

The following is an *API design sketch*, not existing Penelope code. Exact names, lifetimes and generics must be settled by a compile-tested public fixture during F1.

```rust
pub trait Machine {
    type State;
    type Event;
    type Effect;
    type Error;

    fn transition<Sink: IntentSink<Self::Effect>>(
        &self,
        current: &Self::State,
        event: &Self::Event,
        context: &TransitionContext,
        intents: &mut Sink,
    ) -> Result<Transition<Self::State>, Self::Error>;
}

pub enum Transition<State> {
    Accepted { next: State },
    Rejected { reason: RejectCode },
    Unhandled,
}
```

`TransitionContext` contains immutable, **supplied** logical information only: exact machine scope, definition fingerprint/schema, expected revision and (when committed) stable event identity, along with an application-provided logical-time or deterministic-input descriptor where required. **No ambient clock, random source, filesystem or network access.** The Rust type system cannot prevent a user's transition handler from performing arbitrary I/O; purity must be documented and checked in qualification fixtures.

Required properties:

- All state changes are returned as a candidate value, never by mutating the published state.
- A guard refusal is a typed result. `Unhandled` means no handler accepted the event; it is **not** the same as `Rejected` or an execution error.
- Tentative effects produced before a rejection/error are dropped; they never reach the authoritative outbox.
- `Machine::transition` has no blanket `Clone`, `Serialize`, `Send`, `Sync`, `std`, or `alloc` bound. Concrete storage profiles add only their necessary bounds.
- A successful `Transition` does not mean it is committed. A `PreparedTransition`/`ReadyTransition` token binds the expected head and the exact candidate; only the machine owner or durable store may publish it.
- `MemoryMachine::handle` owns published state and swaps it only after all validation succeeds. Reading state is borrowing, not exposing mutable references that bypass transitions.
- Keep source error types precise; use typed `thiserror` wrappers for infrastructure failures, preserving application errors in generic variants.

**Performance note:** Returning an owned next state can require copying complex application state. That is acceptable for initial modest state machines but must be measured. A future delta/transactional state-edit extension may operate on isolated caller-provided scratch. Do **not** silently add `Clone` to the base trait or promise zero-copy state replacement for arbitrary types.

### 4.2 First-class states and events

Use normal Rust structs/enums, not a repeated string-valued graph schema. State-local data should be encoded with enum variants, so impossible *structural* combinations cannot be constructed casually:

```rust
// Proposed 0.5.0 consumer model; API names are illustrative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderState {
    Created,
    AwaitingPayment,
    Paid { transaction: TransactionId },
    Shipped { transaction: TransactionId, tracking: TrackingId },
    Delivered { transaction: TransactionId, tracking: TrackingId },
    Cancelled { reason: CancellationReason },
}

pub enum OrderEvent {
    Submit,
    PaymentConfirmed { transaction: TransactionId },
    Ship { tracking: TrackingId },
    Deliver,
    Cancel { reason: CancellationReason },
}
```

For example, a `Created` order cannot simultaneously carry a tracking ID in that type. However, whether `Ship` is legal in a current state is a **runtime transition rule** unless a separate statically typed/typestate entry point is used. The implementation should never claim otherwise.

### 4.3 Guard and rejection semantics

A guard is a deterministic validation step against the current state, event and captured context. It can return:

- **Allow** — proceed to the candidate.
- **Reject(code)** — no state/effect change; provide a stable, bounded code for diagnostics.
- **Unhandled** — permit hierarchical parent fallback (if configured); otherwise refuse without mutating state.
- **Error(typed fault)** — abort evaluation without treating the attempt as a business denial.

Guard ordering must be deterministic and defined, not based on map iteration, concurrency, or implicit handler registration order. Document which guard rejected a transition without persisting secrets or arbitrary message contents. Do not use unchecked string names as transition identity or authorization policy.

### 4.4 Hierarchical state machines (HSM)

0.5.0 should support **single-active-path hierarchy**, not just flat enums:

- A finite active leaf path and explicitly declared parents/superstates.
- Event dispatch from the active leaf outward through parent handlers until handled or explicitly rejected.
- Distinguish **unhandled** (can bubble) from **rejected** (terminal refusal).
- Guards evaluated for the chosen transition; no side effects in guards.
- Deterministic exit actions from leaf to least common ancestor; transition action; entry actions from ancestor to target leaf. Exact self-transition/internal-transition semantics must be specified and tested.
- Entry/exit *effects* are staged intents, never executed during evaluation. If any stage fails, the complete candidate is discarded.
- Stable typed state/transition identifiers for provenance (not user-authored string IDs); an enum-based `StatePath` interface may provide the relationship. Avoid runtime reflection requirements.
- Cycles and repeated events are permitted but bounded by the caller's per-dispatch depth/step/intent budgets; no accidental infinite implicit transition loops.
- No orthogonal/parallel regions in 0.5.0; this must be explicit in docs rather than pretending nesting covers all Statig HSM features.

A hierarchy should be an optional helper atop the same `Machine` contract. Flat enums and hand-written matches must remain trivial and fast.

### 4.5 Compile-time transition safety (honest boundary)

Offer structural safety with enum variants and exhaustive matches immediately. An optional typed transition/typestate facade may restrict **static** calls to legal edges, e.g. `Order<Created>::submit() -> Order<AwaitingPayment>`, but it cannot replace dynamic event dispatch over restored state or untrusted messages. It is not a release blocker unless an actual user-facing use case requires it; do not import a mandatory macro dependency solely to promise compile-time guarantees that Rust cannot provide for dynamic transitions.

### 4.6 Shared mechanics to extract rather than duplicate

Audit reusable primitives from existing implementations and centralize only when equivalence is proven:

- A candidate-publication protocol (`expected revision -> validated ready token -> CAS/publish`) with distinct in-memory and durable commit backends.
- Bounded intent staging and poison-on-overflow logic. Timeline `EffectSink` currently contains timeline-specific `StepContext`/descriptor semantics; **adapt** it to a portable `IntentSink` or extract a narrow common internal helper while preserving its public type/ABI.
- Typed deterministic refusal/error classification: rejected, stale/conflict, corrupt/incompatible, quota, untrusted/unauthorized, ambiguous effect, recovery required.
- Immutable event identity, sequence and scope validation helpers, without erasing existing saga/task/timeline newtypes.
- Batch replay correctness helpers: ordered sequence, duplicate detection, exact definition identity, replay bounds, rejected record handling.
- Stable definition/schema identity validation and migration admission, without changing old wire representations.
- Common transition-provenance **view** structs that can be constructed from existing saga/task/timeline histories and the new machine history.

**Do not** move every `StepId`, `TaskTick`, and `TimelineRevision` into one generic integer type. Domain distinctions are deliberate safety barriers.

---

## 5. One complete consumer API and lifecycle

### 5.1 Canonical namespaces

Public facade (target, canonical):

```text
penelope::machine     state/event machine, transition decisions, HSM, in-memory owner
penelope::saga        linear/graph workflows, step effects, retries, compensation
penelope::task        recoverable task and supervisor contracts
penelope::timeline    optional bounded speculative/corrective history
penelope::storage     durable machine/process/task ports, journals and outbox contracts
penelope::runtime     runtime integration traits, runtime-independent controls
penelope::integration::statechronicle  canonical event verification boundary
penelope::diagnostics structured transition and effect provenance views
```

The old `penelope::executor`, root `decide`/`start`/`apply_graph_result`, and existing `task`, `timeline`, `ports`, `statechronicle` reexports remain reachable through 0.5.0 unless an explicit source-break decision is made and documented. No hidden `pub use` aliasing that changes event types or recovery behavior. New docs teach the canonical names; a migration table specifies the old equivalents.

### 5.2 Canonical operations

For local machines:

```rust
let machine = MemoryMachine::new(definition, initial_state, limits);
let outcome = machine.handle(event, context)?;
// Accepted => new state visible; Rejected/Unhandled => unchanged.
```

For durable machines:

```rust
let current = store.load_exact(scope, revision).await?;
let candidate = model.prepare(&current, event, context)?;
let receipt = store.commit(candidate.into_commit_request(codec)?).await?;
let published = receipt.verify_against(&candidate)?;
// Only now are committed intents eligible for authorized dispatch.
```

These are **pseudocode**, not implemented APIs. Actual API must avoid a lifetime/ownership model that prevents safe use of a prepared transition or permits consuming it twice. A high-level convenience method may wrap the sequence, but low-level prepare/commit/receipt verification must remain available and testable.

### 5.3 One explicit record of what happened

A canonical diagnostic view should answer:

```text
machine scope / instance            exact tenant + machine + definition
previous state path and revision    validated before-state
accepted event                     type + stable event identity + payload digest
selected transition                stable ID + guard outcome
next state path and revision        validated after-state
commit result                      committed / duplicate / stale / unknown
planned intents                    stable identity and payload digest, redacted details
observed outcomes                  accepted / committed / dispatched / verified / unknown
source causation                   original request or parent event, when available
```

No new universal public `StateChanged` enum should be forged for old histories. Implement **lossless adapters** from existing records, including “not recorded in this legacy protocol” when a field is absent. Operational logs are not authoritative; durable provenance must derive from immutable committed records. Rejected events may be separately audited through a bounded authorized diagnostic port, but they must not increment the machine state sequence unless a deliberately specified accepted-no-op event mode is enabled.

---

## 6. Integrate saga, task, timeline — actual refactor work

### 6.1 Saga: built-in coordinating machine

Existing public code: `crates/penelope-executor/src/engine.rs`, `graph.rs`, `graph_outcomes.rs` and domain/ports types.

Work:

1. Introduce `penelope::saga` as the canonical facade for the existing linear and graph engines.
2. Implement an adapter that exposes each legacy `LinearSagaProjection`/`GraphSagaProjection` as the state of a specialized machine; map legacy accepted events and `SagaDecision` into the shared prepared-transition *view*.
3. Preserve `LinearSagaDefinition`, `ProcessGraphDefinition`, `StepPlan`, `GraphTransition`, `SagaStatus`, action identity checks, compensation, timers, manual review and all existing error meanings.
4. Extract graph shape checking (bounds, reachability, duplicate edges, and step identity) into shared **internal** deterministic helpers if they demonstrably serve HSM definitions; keep graph's result-category logic as a specialization.
5. Preserve fixed legacy step limits, transition limit `MAX_GRAPH_TRANSITIONS = 512`, visit limits and bounded replay. Do not widen or change a limit without separate compatibility and performance justification.
6. Reuse the new machine effect lifecycle vocabulary at the facade/adapter level, while preserving the committed saga `ProcessAction` and `AtomicProcessCommit` wire records unchanged.
7. After parity qualification, remove duplicate orchestration-level glue and redundant validation pathways; avoid replacing optimized legacy executor loops with trait-object dispatch that regresses performance.

**Critical:** The legacy graph saga does **not** become a general application HSM just because it has transitions. New HSM transitions belong to `machine`. A saga can be driven by events produced by a machine or can coordinate machine-intended effects, with exact typed bridging rather than mutually recursive hidden calls.

### 6.2 Task: built-in recoverable lifecycle machine

Existing public code: `penelope-core/src/task/{contracts,projection,control,runtime}.rs`, `penelope-executor/src/task.rs`.

Work:

1. Expose `TaskProjection`/`TaskRecord`/`TaskExecutionEvent` through a machine-compatible transition inspection and decision adapter without altering their v1 encodings.
2. Maintain the copied-candidate `TaskProjection::apply()` semantics and the `TaskControl` journal-first start/restart rule.
3. Keep task generation, checkpoint schema, effect ambiguity, reconciliation, recovery modes, execution budget and fencing as task-specific types. A generic `Machine` must **not** give users a bypass around these preconditions.
4. Share pure event validation and structured provenance infrastructure where types align, but preserve the no-alloc task core, fixed record sizes, and supervised runtime behavior.
5. Ensure a machine's committed effect may intentionally start a recoverable task only after a typed, verified admission binding. A task result becomes a new event after scope/generation/effect evidence checks—not a silent state mutation.
6. Where machine and task journals are in different stores, use an idempotent saga/outbox/inbox bridge; do not promise one cross-store atomic commit.

### 6.3 Timeline: optional history/correction policy

Existing public code: `penelope-core/src/timeline/*`, `penelope-timeline/src/{contracts,step,session,live,history,effects,handoff}.rs`.

Work:

1. Make a machine's deterministic transition model adaptable to `RewindableComputation`, including its `Input`, `State` and `Effect` roles.
2. Reuse `prepare_live_step`, `LiveTimeline`, bounded `ReplaySession`, `FixedHistory` and `EffectHandoff`; avoid a second snapshot or correction engine inside `machine`.
3. A machine event may become a timeline input if its **entire computation dependency set** is captured and versioned. Do not default every machine to “rewindable” solely because it is deterministic in a trivial example.
4. For speculative inputs, produce **tentative** effect intents; only the 0.4.0 independently finalized prefix may be handed off. Keep correction across irreversible effects forbidden by the existing frontier rules.
5. Pinned definition/schema versions must match across the correction horizon. Cross-version history migration requires an explicitly reviewed transformation, not reinterpretation.
6. Keep the default 0.4.0 timeline supported profile: in-memory owned state / optional shared lock, immutable bounded history, user-owned complete snapshots. A persistent full-timeline adapter is **not** established by the new machine SQLite journal.
7. Avoid double journaling the same accepted event through unrelated persistent histories. A consuming integration must designate one authority for each record and establish an explicit correlation with any timeline history.

### 6.4 One compositional example, not hidden recursion

```text
Order machine receives Submit
   -> commits AwaitingPayment + RequestPayment intent
   -> durable outbox dispatch starts/advances payment saga
   -> saga drives recoverable task for external payment call (if configured)
   -> unknown result => task/saga reconciliation; order stays AwaitingPayment
   -> verified payment evidence => PaymentConfirmed machine event
   -> order transitions to Paid

Optional timeline:
   -> local tentative order projection may be corrected/replayed
   -> externally visible payment is never repeated merely because history rewound
```

No component directly mutates another's state as a hidden side effect. All cross-component causal edges are typed, admitted events with exact bindings.

---

## 7. Durable machine protocol and storage

### 7.1 Separate, minimal, versioned machine protocol

Introduce `penelope.machine.v1` **independent** of saga v1, task history v1, and `penelope.timeline.v1`.

Candidate durable record fields (contract to finalize in F1):

```text
protocol_tag/version
machine_scope              tenant + machine ID + definition ID/fingerprint
model_schema               pinned compatible state/event schema identifiers
sequence/revision          exact per-instance predecessor and next sequence
accepted_event_id          unique stable per-instance event identity
accepted_event_type        stable typed numeric code / schema identity
accepted_event_digest      hash over canonically encoded immutable event payload
previous_state_digest      hash/identity of committed prior state (if applicable)
next_state_digest          hash/identity of committed resulting state
transition_id              stable transition identity, if exposed by the model
causation                  typed optional source identity and correlation
intents                    bounded, ordered, individually stable IDs/digests
commit_identity            deterministic token bound to the whole accepted batch
```

Persist the exact encoded event bytes or an authenticated immutable blob reference as required for replay. Record ordering and decoding must be bounded and validated. Do not confuse a hash with authentication; a maliciously rewritten store can rewrite both bytes and hash unless protected by a separate trust boundary.

### 7.2 Atomic commit semantics

`MachineStore::commit` must be a *single local transaction* with these guarantees:

1. Check the exact machine scope and pinned model/schema fingerprint.
2. Validate the expected revision/CAS and event uniqueness under the transaction lock.
3. If the same event ID and same immutable payload/commit identity already exist, return a verifiably identical duplicate receipt, not an additional transition.
4. If the same ID exists with different content, reject with conflict. Never treat it as idempotent.
5. Atomically commit accepted event, new revision/head and state identity, replay payload/storage references as required, and **all** outgoing intent records. All-or-nothing.
6. A rejected/unhandled evaluation must not call the transition-commit path as though it succeeded.
7. Ensure a crash after commit and before receipt can be resolved by reading authoritative persisted records, not by guessing whether the action succeeded.
8. Never dispatch an effect before the **verifiable committed receipt** and authorization checks.
9. Enforce scoped quotas, record-size limits, history retention, resource bounds, and integer overflow prevention before irreversible partial mutation.
10. Preserve a coherent read of state/head/event for concurrent readers; a state-byte payload and the revision that describes it cannot become independently visible.

### 7.3 New port contracts

Add `penelope-ports/src/machine.rs` for narrow types/traits such as:

- `MachineStore` — atomic transition admission, `load_head`, bounded paginated history, exact committed-record lookup by event ID, and verified receipt semantics.
- `MachineCodec` — application-supplied bounded canonical encoding and schema-aware decoding of state/event payloads, separated from the pure core.
- `MachineIntentOutbox` — optional bounded claimed/acknowledged intents, stable identities and effect reconciliation; internally reuse generic worker/lease helpers where proven equivalent.
- `MachineAuthorizer` (or precise reuse of existing authorization policy interface through a typed adapter) — validates host-authenticated caller/context for committing actions and consuming effects.

Do not grow the legacy `ProcessAuthorizationOperation` enum or change task wire types by convenience alone: old exhaustive match compatibility and authorization defaults need a separate review. New machine operations may use a new, domain-specific authorization enum and an adapter into host policy.

### 7.4 SQLite integration

Target: `crates/penelope-sqlite/src/machine.rs` plus a **versioned, forward-only** SQLite schema migration in `schema.rs` (or submodule).

Suggested *logical* tables, names to be finalized against existing migrations:

```text
machine_heads          primary key(scope), definition/schema, sequence, state reference
machine_events         unique(scope, sequence), unique(scope, accepted_event_id)
machine_payloads       immutable scope+digest+schema payload storage
machine_intents        unique(scope, intent_id), committed status/delivery metadata
machine_snapshots      optional bounded checked snapshots, when configured
```

Reuse the actual `DatabaseWorker`, WAL/FULL setup, transaction practices, bounded record/blob read helpers, receipt validation patterns, recovery tests and quota charging where appropriate. Avoid copying the entire saga SQL engine into parallel `machine_...` code. Shared generic SQL machinery may live in a private adapter module; keep domain-specific queries and tables domain-specific.

Use SQLite transactions on the bounded worker, not direct blocking calls on an async reactor. Validate both competing writers and restart after an ambiguous commit. SQLite must remain optional and never enter the `no_std` dependency graph.

### 7.5 Recovery and snapshots

Define two supported persistence profiles clearly:

- **Event replay profile:** replay exact accepted events under the original pinned model/schema, verify sequences/identity/expected state hashes, and reconstruct the current state. No effects executed while replaying.
- **Snapshot-accelerated profile:** an atomically bound verified snapshot reduces replay cost. Snapshot retention and migration rules must never allow old code to reinterpret new schema bytes, or a state to be published without its corresponding head.

Do not claim that the standalone machine journal is a durable timeline branch/correction store. Reconciliation for external effect status is independent of both snapshot profiles. Require code-version/definition pinning sufficient to reproduce old transitions; if the executable is unavailable, fail closed with a typed migration/recovery-required result.

### 7.6 StateChronicle and canonical ownership

A Penelope machine may represent a derived view or controller for a StateChronicle-backed domain. It must not become a competing authority over balances, ownership, positions, or settlement. Verified canonical events must bind tenant, operation, resource, action and actual committed evidence. Cross-database publication uses idempotent correlation, not an invented global transaction. A machine record must distinguish locally committed projection from externally authoritative fact.

---
## 8. Concrete codebase refactor map

This section maps proposed changes to files and modules that **actually exist in 0.4.0**. New paths are marked **NEW**. Keep `lib.rs`/`mod.rs` as declaration/export files to conform to existing repository architectural checks.

| Current location | 0.5.0 action | Ownership after refactor |
| --- | --- | --- |
| `crates/penelope-core/src/lib.rs` | Add `pub mod machine` (**NEW**) | Portable shared transition contract and evaluator |
| `crates/penelope-core/src/task/{contracts,projection,control}.rs` | Integrate shared candidate/provenance adapters; retain existing task wire and checked state transitions | Task-specific recovery, supervisor and lifecycle rules |
| `crates/penelope-core/src/timeline/{contracts,projection,replay_plan,wire}.rs` | Reuse stable model/identity validation helper only where safe; retain codec and frontier | Independent timeline protocol/rewind contracts |
| `crates/penelope-core/src/machine/{contracts,decision,hierarchy,projection,identity}.rs` (**NEW**) | Define public `Machine`, `Transition`, `TransitionContext`, bounded intent/candidate contracts and optional HSM helpers | Single pure machine model |
| `crates/penelope-domain/src/implementation.rs` | Leave existing saga DTO serialization and identifiers untouched initially; expose internal validation helpers selectively | Legacy saga/domain wire contracts, not a dumping ground for generic machine types |
| `crates/penelope-executor/src/engine.rs` | Refactor behind canonical saga facade/adapters; keep old functions/reexports | Linear saga behavior, versioned legacy semantics |
| `crates/penelope-executor/src/graph.rs` | Deduplicate common graph validation and event/transition helper logic where parity proves safe | Specialized graph saga semantics and replay |
| `crates/penelope-executor/src/task.rs` | Expose typed machine-to-task effect composition; preserve hosted task supervisor | Application execution composition |
| `crates/penelope-executor/src/machine.rs` (**NEW**) | Canonical application prepare/publish/commit orchestration; in-memory convenience and durable coordination where appropriate | Hosted/high-level machine composition, no storage implementation |
| `crates/penelope-ports/src/implementation.rs` | Retain v1 port definitions and direct reexports; factor private common receipts/validation helpers carefully | Existing process ports and compatibility |
| `crates/penelope-ports/src/machine.rs` (**NEW**) | Machine commit/journal/codec/authorization/intent port contracts | Backend-neutral durable machine interface |
| `crates/penelope-sqlite/src/{schema,process,tasks,worker}.rs` | Reuse worker, WAL/FULL, quota/atomicity infrastructure; no changes to old record semantics | Adapter shared infrastructure and existing domains |
| `crates/penelope-sqlite/src/machine.rs` (**NEW**) | Machine journal, transactional CAS, history, optional snapshots/outbox | Qualified single-host durable machine adapter |
| `crates/penelope-timeline/src/{contracts,live,session,step,effects,handoff}.rs` | Add narrow `Machine` model adapter if dependency boundaries permit; otherwise implement adapter in an outer layer | Existing timeline implementation, without new core copy |
| `crates/penelope/src/lib.rs` | Canonical `machine/saga/task/timeline/storage/runtime/diagnostics` exports and retained compatibility imports | **Single** public developer-facing entry point |
| `crates/penelope/Cargo.toml` | Add only required optional feature gates; keep `default=[]` unless separately justified | Minimal default features |
| `Cargo.toml`, `Cargo.lock` | 0.5.0 workspace version, new source modules and necessary dependencies only | Reproducible build |
| `scripts/check-layer-boundaries.sh` | Update exact allowlisted dependencies for new modules without loosening checks | Enforced modular structure |
| `scripts/publish-order.py` | Keep inferred dependency-first packaging; add no manually ordered duplicate publish list | Release automation |
| `docs/RELEASE.md`, `README.md`, `CHANGELOG.md` | Rewrite documentation around unified model and preserve historical release record | Coherent public product explanation |

**Prefer not to create additional publishable crates.** The current ten-library-crate topology can support this refactor. If an optional procedural macro is later accepted, a small `penelope-macros` crate is an exception only after the trait-based API and MSRV/build impact are qualified. Neither a macro nor SQLite is a prerequisite for a complete in-memory state machine.

### 8.1 Shared code extraction rules

A refactor is accepted only when:

- At least two real implementations use the extracted component (not a hypothetical future consumer).
- The extraction removes duplicate semantics or code, not merely moves a `match` into a generic callback.
- No previously validated invariant becomes the responsibility of an untrusted downstream adapter without an explicit safety-boundary change.
- Old event/record encodings and failure categories retain their meaning.
- Generic dispatch does not induce unnecessary allocations, heap indirection or virtual calls on existing fast paths.
- A dependency boundary test catches any future reversal of the agreed layering.

Example: share a bounded `PreparedTransition` lifecycle and provenance *interface*; do **not** replace `TaskProjection`'s explicit journal record phase with the timeline's speculative revision phase, because their meanings differ.

### 8.2 API naming and source migration policy

Prefer names that reflect what users are doing (`Machine`, `Event`, `State`, `Transition`, `handle`, `prepare`, `publish`, `replay`, `inspect`) rather than the current internal crate (`engine`, `graph`, `implementation`). Do not introduce untyped string-based `transition("Paid")` APIs, “one global scheduler”, or overly clever builder macros.

Two stable categories of API:

- **Canonical 0.5 API:** concise, domain-specific public modules with documentation/tests and safe default contracts.
- **Legacy 0.4 API:** public paths retained where feasible, with explicit migration guidance. Do not use a breaking rename as evidence of actual architectural consolidation.

Do not mark legacy functions deprecated until the new canonical alternatives offer **equivalent behavior**, including effect handling and failure/recovery semantics. Deprecation timing should follow a stated compatibility policy, not be mixed into the performance refactor by default.

---

## 9. End-user ergonomics: state machines genuinely comparable to Statig

### 9.1 Basic user implementation

The minimum successful user experience must be a single Rust state enum, a single Rust event enum, and one transition implementation. No SQL, state IDs typed as strings, registration boilerplate, scheduler, or duplicate graph table.

```rust
// Illustrative target API. Compile-check and refine it before documenting
// it as a public example.
use penelope::machine::{Machine, Transition, TransitionContext, IntentSink};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Door {
    Closed,
    Open,
    Locked,
}

enum DoorEvent {
    Open,
    Close,
    Lock,
    Unlock,
}

struct DoorMachine;

impl Machine for DoorMachine {
    type State = Door;
    type Event = DoorEvent;
    type Effect = ();
    type Error = core::convert::Infallible;

    fn transition<S: IntentSink<Self::Effect>>(
        &self,
        state: &Door,
        event: &DoorEvent,
        _context: &TransitionContext,
        _intents: &mut S,
    ) -> Result<Transition<Door>, Self::Error> {
        let next = match (state, event) {
            (Door::Closed, DoorEvent::Open) => Door::Open,
            (Door::Open, DoorEvent::Close) => Door::Closed,
            (Door::Closed, DoorEvent::Lock) => Door::Locked,
            (Door::Locked, DoorEvent::Unlock) => Door::Closed,
            _ => return Ok(Transition::Unhandled),
        };
        Ok(Transition::Accepted { next })
    }
}
```

This demonstrates structural state modeling, explicit permitted edges, and a lightweight pure decision function. In real implementations `Unhandled` and business `Rejected` may be configured separately. Add a realistic example that exercises error/guard behavior and effects rather than only the happy path.

### 9.2 HSM ergonomic profile

A second example must model a *nested* state machine: e.g. `Connection::{Disconnected, Connecting, Connected::{Idle, Active}}`, with a parent-level `Disconnect` event, child-level message handling, guard rejection, and deterministic exit/entry tracing.

Acceptance criteria:

1. State hierarchy is expressed with Rust types/traits; user authors each state and event **once**.
2. No repeating state names in several string transition tables.
3. Parent fallback and leaf override are explicit and predictable.
4. Exit/entry order is asserted by runnable tests, including self-transitions, parent transitions, and guarded failures.
5. State-local data cannot appear on the wrong enum variant.
6. Runtime restoration validates that the active nested path is structurally legal.
7. The API can be used with an unboxed, allocation-free nested enum where application state permits.
8. It is possible to inspect a rejected transition with its source leaf, attempted event, and guard code.

A public macro is optional sugar, not the only way to define hierarchy. Do not copy Statig's surface syntax just to claim feature equivalence; select a Rust-native API that works for the verified use cases. Provide explicit documentation of unsupported orthogonal states.

### 9.3 Runtime vs compile-time guarantees

Document guarantees in three tiers:

- **Compile-time:** well-typed State/Event/Effect, enum-associated state-local data, exhaustive user matches, generic separation of event/effect types, optional typestate for static workflows.
- **Runtime determinism:** guard and transition checks, rejection of illegal dynamic events, one chosen transition per dispatch, deterministic HSM ordering and bounded internal processing.
- **Durable fault tolerance:** exact event identity and revision CAS, verified commit, reconstruction without re-executing effects, idempotent outbox/admission and task-specific reconciliation.

Never collapse these tiers into a claim that all invalid state transitions are unrepresentable. Incorrect application domain predicates and hidden nondeterminism remain consumer hazards unless explicitly verified.

---

## 10. Cross-cutting correctness and security contract

Each invariant must have an assigned implementation owner and a negative test; do not leave it implicit across crates.

| Invariant | Owner | Failure behavior |
| --- | --- | --- |
| Candidate never mutates accepted state | `penelope-core::machine`, memory/durable composition | Discard candidate; live state unchanged |
| Refused/failed event never dispatches effects | Core `IntentSink`, publication layer | Drop/poison tentative intent list |
| Accepted event has exact scope/definition/schema | Machine contracts + codec/store | Typed incompatibility error |
| Duplicate event ID with different bytes fails | Machine journal/SQLite | Conflict, no new state/effect |
| Stale expected revision cannot publish | Memory owner or durable CAS | Conflict, caller must reload/re-evaluate |
| Replay performs zero outbound I/O | Replay adapter and composition | Reject impure callbacks in test harness; never call dispatcher |
| Definition migration cannot reinterpret old state | Definition registry and storage codec | Explicit migration required |
| Timeline correction cannot reverse committed effects | Existing timeline finality | Refuse correction or require separate compensating workflow |
| Task retry cannot bypass effect reconciliation | Existing task supervisor/ports | Remain blocked on unknown outcome |
| Saga result cannot be substituted across action IDs | Existing graph/linear saga guards | Typed action/scope mismatch |
| Canonical external event is independently verified | StateChronicle bridge | No canonical assertion on local state alone |
| Resource consumption is bounded | Core capacities, storage limits, runtime budgets | Quota/capacity failure before publication |
| Cross-tenant information does not leak through diagnostics | Host authorization + redacted views | Deny/read-only redacted response |
| PANIC/unwind during evaluation never leaks a staged published state | Scratch owner and RAII intent/session cleanup | Candidate unusable; authoritative state unchanged |
| Ambiguous commit/dispatch ACK is not treated as success or failure | Store + effect ports | Reconstruct/read authoritative record; reconcile effect independently |

Additional adversarial cases:

- Forged `machine_scope`, schema/definition digest, event ID or commit receipt.
- Wrong `expected_revision`, counter wraparound, truncated event log, reordered history, duplicate sequence, altered intent payload.
- A parent HSM handler accidentally catches an event after an explicit child rejection.
- Guard evaluation based on ambient clock, RNG, external mutable state or nondeterministic hash iteration.
- A transition produces more intents than capacity or signals an error after staging effects.
- Receipt lost after full SQLite commit; second writer races the same event; delayed prior generation submits an effect result.
- Malformed snapshot or invalid schema migrated without permission.
- Application code panics between candidate preparation and publication; callback reentrancy attempts another transition on the same owner.
- A timeline correction that changes a prior state but would reproduce an already-finalized outgoing payment.
- Two storage backends (machine and saga/task) accept separate writes and one crashes: ensure the bridge is recoverable, not falsely advertised atomic.

### 10.1 Authorization boundaries

The pure `Machine` contract does **not** authenticate a caller. A hosted entry point must establish the principal from a trusted transport or execution context, authorize machine/event operation, and pass only validated, immutable input to the transition function. A digest, scope ID or replay record is not proof that an action was authorized. Privileged administrative transitions and reconciliation overrides require separately documented policies. Sensitive payloads and guard internals must not appear in default diagnostic logs.

### 10.2 Source-of-truth policy

A durable `MachineStore` is authoritative **for Penelope machine-state history in its own scope**. This does not make it authoritative for external payment settlement, StateChronicle ownership, a bank account, or another service's resource. An external fact must come from that domain's authenticated canonical evidence. The phrase “database authoritative is bad” is not a useful general rule: the correct rule is **each domain has an explicitly selected authority, and adapters may only claim the guarantees of that authority**.

---

## 11. File-by-file implementation phases and pull requests

The following plan is dependency ordered. **No version bump or release tag until the end.** Every phase lands with matching tests and docs, and every extraction demonstrates parity to 0.4.0. If schedule pressure appears, reduce optional features, not safety gates or migration integrity.

### F0 — Capture the 0.4.0 baseline (must precede refactoring)

**Purpose:** Establish an actual reproducible baseline, not infer performance/correctness from the existence of tests.

**Work:**

- Record release tag `v0.4.0` and HEAD SHA, toolchain versions, host characteristics, `cargo metadata`, public API exports, current feature matrix, release crate list and existing protocol golden fixtures.
- Run the existing workspace/MSRV/portable/CI-equivalent checks, full selected chaos/fuzz/coverage profiles, current saga/task/timeline examples, and representative performance benches.
- Record any failing pre-existing test, expected latency exception, nondeterminism, or environment limitation as an explicit baseline deviation. Do **not** erase a failing test just to make 0.5 appear green.
- Create a dependency/semantics map for `penelope-core`, `domain`, `executor`, `ports`, `sqlite`, `timeline`, and `process-runtime`, including all public reexports that downstream users may import.
- Freeze copies of old protocol test fixtures and a baseline API compile harness as external sample crates.

**Files:** `docs/0.5/baseline.md` (**NEW**), `docs/0.5/compatibility-matrix.md` (**NEW**), CI or reproducible bench scripts, no production behavior change.

**Gate F0:** Baseline is command-reproducible, limitations are recorded, and old-version fixtures can be checked against new code in later phases.

### F1 — Define unified contracts and compatibility policy

**Purpose:** Decide semantics before moving implementation. Avoid creating another ad hoc state engine.

**Work:**

- Approve the state/event/transition, guard, rejection, HSM, intent, prepared-candidate and commit life cycles from sections 3–7.
- Define the precise distinctions between `Rejected`, `Unhandled`, evaluation fault, conflict, durable commit ambiguity and effect ambiguity.
- Specify `MachineScope`, definition fingerprint, schema identity, accepted event IDs, sequence/revision, stable transition identity and provenance view, with no unsafe implicit conversions from task/saga/timeline IDs.
- Define feature matrix and `no_std` rules; explicitly choose whether HSM uses borrowed constant-size state paths or a caller-supplied workspace. Resolve const-generic capacity behavior and maximum nesting/depth budgets.
- Write **compile-tested** small, hierarchical, legacy-saga-adapter and legacy-task-adapter public API fixtures before implementing everything.
- Define machine protocol version 1 and a canonical byte encoding strategy that is deterministic, bounded and testable. Avoid conflating canonical encoding with default Serde JSON output from unrelated old protocols.

**Files:** `docs/0.5/architecture.md` (**NEW**), `docs/0.5/machine-wire-v1.md` (**NEW**), `docs/0.5/public-api.md` (**NEW**), API test fixtures.

**Gate F1:** A reviewer can trace the entire user request from event admission to candidate, commit, effect authorization, replay and diagnostics without finding two contradictory definitions of “accepted”, “committed” or “finalized”.

### F2 — Implement the portable machine core

**Purpose:** Make arbitrary application machines a lightweight first-class component.

**Work:**

- Add `penelope-core/src/machine/*` modules and public `Machine`, `Transition`, `TransitionContext`, typed refusal/error, `IntentSink`, `PreparedTransition`, `MemoryMachine` (or a minimal wrapper in the correct layer).
- Implement deterministic state evaluation, staging, validation, rejection, safe publication and core provenance.
- Keep flat-machine API allocation-free for allocation-free consumer types; require no `Clone` or serialization in base trait.
- Add state-path/HSM support with explicitly bounded depth and well-defined parent fallback/guard/entry/exit behavior; compile-test this against the target MSRV.
- Use no mandatory async runtime, SQLite, saga, task or timeline dependency.
- Add a machine-specific `no_std` RISC-V build and allocation probe, not just a host `cargo check`.

**Tests:** transition truth tables, attempted illegal edges, parent bubbling, guard refusal, panic safety, rejected staged-intent poison/drop, exhaustive depth, edge-specific state-local data, deterministic replay oracle and no-heap profile.

**Gate F2:** A user can build a flat and nested machine via the `penelope` facade alone; all state changes are explicit, testable, and do not depend on storage. Compiled examples are the published API, not aspirational pseudocode.

### F3 — Consolidate saga and graph into the unified model

**Purpose:** Integrate what Penelope already does, rather than expose a separate engine to users.

**Work:**

- Create canonical `penelope::saga` reexports and a machine-compatible saga projection/decision adapter.
- Introduce shared private graph/provenance/validation helpers only after differential tests prove identical decisions and failure modes.
- Preserve legacy `LinearSagaEvent`, `GraphSagaEvent`, definition digests, step ordering, `ActionResult::Unknown`, compensation and manual review.
- Keep fast public functions available through compatibility reexports; do not introduce extra validation passes on the hottest step path unless measured and justified.
- Update trade, graph, compensation and retry examples to show how a saga is a specialized machine and how an application machine may coordinate with one.

**Tests:** generate identical legacy inputs/history under 0.4 and 0.5 and compare projections, next actions, statuses, errors, record bytes and replay (including graph limits/invalid edges/unknown effects).

**Gate F3:** Every previous saga supported case returns the same outcome with no wire changes, and the new facade composes sagas without introducing an alternate execution authority.

### F4 — Consolidate task supervision into the unified model

**Purpose:** Expose the recoverable task lifecycle consistently while retaining safety-specific behavior.

**Work:**

- Provide machine-compatible task inspection/transition adapters and common provenance, with distinct `TaskStatus` and task effect results intact.
- Refactor duplicated candidate validation carefully; keep `TaskProjection::apply` atomic and `TaskControl`/`TaskSupervisor` recovery semantics unchanged.
- Implement typed machine-intent-to-task launch and verified task-result-to-machine-event composition with explicit identity binding and idempotency.
- Retain generation fencing, checkpoint migration proof, journal commit before execution, unknown-outcome reconciliation and protected worker profile.
- Ensure the plain in-memory machine does not pull in task runtime dependencies.

**Tests:** existing portable task/SQLite worker fuzz, injected failures, real-thread arbitrations, stale generations, checkpoint corruption, poisoned supervisor, missing/ambiguous receipts and task-effect bridge.

**Gate F4:** All 0.4.0 task histories are still reconstructible and accepted only under the same conditions. A common machine interface cannot authorize a forbidden task retry or skip reconciliation.

### F5 — Integrate timeline without cloning its machinery

**Purpose:** Make rewindable state an optional mode over deterministic machines.

**Work:**

- Add an adapter between user `Machine` transitions and `RewindableComputation` that preserves isolated scratch and staged intents.
- Reuse existing timeline instance ownership, `FixedHistory`, reservations, candidate validation, head CAS and effect finality.
- Add explicit tests distinguishing live canonical transitions from tentative timeline corrections and effect-finalized history.
- Document what happens when a machine definition/schema changes across a replay window, and when data dependencies cannot be captured.
- Do not promise a persistent correction adapter as a side effect of adding machine SQLite persistence.

**Tests:** existing `penelope-timeline` property/fuzz campaigns plus hierarchy replay, invalid correction, missing predecessor, stale head, branch exhaustion, effect-finalized-prefix refusal and client prediction sample.

**Gate F5:** All 0.4.0 timeline examples remain valid, new machine examples can use the optional timeline, and the shared code is smaller/clearer than implementing another correction engine.

### F6 — Add machine durability and cross-subsystem effects

**Purpose:** Deliver the value of Penelope's reliability infrastructure for user-defined machines.

**Work:**

- Add the independent machine event/provenance/intent protocol to `penelope-ports` and the core as appropriate to no-std separation.
- Implement a bounded `MachineStore` and local `penelope-sqlite` adapter with a separately versioned migration, verified receipts, duplicate admission, competing CAS, loss-of-receipt reconstruction, scoped quotas and source-authorized outbox dispatch.
- Share existing SQLite worker, transaction, checksum, immutable blob, resource budgeting and lease helpers **by extraction**, not code duplication.
- Add machine-to-saga and machine-to-task durable bridges using exact bound identities, committed outbox/intents and separately acknowledged downstream acceptance.
- Provide a local-memory journal implementation for tests/examples only if useful; do not accidentally imply it is crash-durable.
- Preserve SQLite's single-host local-disk profile; no implied clustered durability or universal exactly-once delivery.

**Tests:** process-kill at precommit/postcommit/pre-receipt/pre-dispatch/post-dispatch stages; double writers; corrupted history; wrong tenant; duplicate event; concurrent reopen; unavailable history; quota exhaustion; old SQLite schema reopen/migration; async queue overflow and blocking I/O isolation.

**Gate F6:** An application-defined machine can crash and recover exact committed state plus pending authorized intents, and an ambiguous external effect never yields a guessed success or automatic unsafe restart.

### F7 — Refactor facade, docs, tests, performance and maintenance surface

**Purpose:** Finish the cohesive product rather than leaving old and new layers side by side indefinitely.

**Work:**

- Refactor facade into canonical namespaces; retain valid legacy reexports and all old examples/compile harnesses.
- Deduplicate verified internal validation/storage/diagnostic helpers. Remove unreachable compatibility **internals** only after tests prove they have no external behavior requirement; do not delete old wire decoding.
- Rework README to introduce: *machine -> optional durability -> saga/task effects -> optional timeline* in one example progression.
- Add one integrated example with model definition, invalid transition, durable commit, task/saga effect, ambiguous ACK, crash/recovery and provenance inspection.
- Add a comprehensive compatibility document and migration guide from 0.4.0.
- Update architecture boundary checks and crate-package validation. Require new code paths to use canonical helpers instead of accruing further duplicate pathways.
- Measure performance and maintenance cost before approving any “cleanup” that removes tested guarantees.

**Gate F7:** An engineer unfamiliar with internals can use the whole framework through `penelope` docs; three specialized subsystems compose through one event/effect story; no legacy guarantee is lost.

### F8 — Release qualification and publication

**Purpose:** Make 0.5.0 a controlled production-quality release.

- Re-run full CI/qualification and compare against frozen 0.4.0 baseline using equivalent benchmark workloads.
- Validate all crate packages on the exact release candidate, lockfile, MSRV, release build and external usage fixtures.
- Review public API source compatibility, independently versioned machine wire goldens, old process/task/timeline protocol goldens and SQLite migrations.
- Produce a release evidence manifest: commit SHA, tools, environment, seeds, run counts, failures, justifications and performance conclusions.
- Only after explicit authorization, tag/publish `v0.5.0` using existing dependency-first scripts and release workflows. This plan does **not** authorize a release action.

**Gate F8:** The definition of done in section 14 is green on the actual release candidate, not just on an earlier feature branch.

---

## 12. Migration and compatibility strategy

### 12.1 Compatibility matrix

| Surface | 0.5.0 strategy | Validation |
| --- | --- | --- |
| Old linear saga API/imports | Keep functioning; canonical new `saga` path | Compile external 0.4 consumer fixture unchanged |
| Old graph saga API/imports | Keep functioning; new adapter/view | Exhaustive graph parity and replay tests |
| Saga v1 wire and outcome contracts | **No reinterpretation/rewrite** | Golden byte fixtures and cross-version replay |
| Task history v1, checkpoint schemas | **No reinterpretation/rewrite** | Frozen task history, recovery and migration tests |
| `TaskControl` restart/fencing semantics | Preserve exact authority gates | State-model fuzz + subprocess crash tests |
| Timeline `penelope.timeline.v1` metadata | Preserve independent format and correction frontiers | Golden codec, history and finality tests |
| Timeline `LiveTimeline` and facade feature | Preserve, add optional machine adapter | External compile and correction tests |
| `AtomicProcessCommit` and process stores | Retain as saga-specific contract | Existing SQLite/process conformance |
| Existing SQLite databases | Upgrade with forward-only machine schema; no deletion/conversion of existing tables | Open/replay v0.4 sample DB with v0.5 adapter |
| New machine protocol | `penelope.machine.v1`, separately versioned | Independent golden vectors, strict decoder fuzz |
| StateChronicle verified canonical events | Retain correlation/trust contract | Upstream pinned conformance suite |
| Default facade feature set | Keep small; machine core usable by default; timeline remains opt-in | Cargo feature/dependency-matrix tests |
| Linux process worker | Retain optional supported profile | Existing runtime isolation/fencing tests |

An *unavoidable* source change (e.g. a new public enum variant that breaks exhaustive matches) requires a written decision and explicit migration note. Prefer new independent types or non-exhaustive views where this avoids old code breakage. Avoid “semver permits it before 1.0” as a substitute for customer compatibility testing.

### 12.2 Existing deployments

1. Existing task/saga deployments **must not** rewrite their histories merely to use 0.5.0.
2. Continue reading old history with the original protocol-specific decoder and replay path. New in-memory canonical adapters can present these as machine views without persisting a fake machine history.
3. On opening an existing SQLite database, apply machine schema addition in a forward-only migration. Existing process/task tables and constraints remain valid. Migration failure must leave a recoverable, well-defined database state.
4. Keep 0.4-compatible runtime/worker behaviors in place; migration to optional machine-generated intents requires explicit adoption, not an automatic change to task recovery policy.
5. For new user-defined machines, pin model version/schema at initial creation and require an explicit, authenticated state/history migration before changing interpretation.
6. During mixed-version rollout, old nodes **must not** accidentally consume new machine records as saga/task records. Gate new protocol usage by consumer capability or deployment version; reject unsupported records without mutating existing state.
7. On downgrade, old code may be incapable of understanding newly admitted machine records. Document this as a **protocol-aware forward-compatibility limit**; do not imply automatic downgrade support. Provide a pre-deploy backup/test gate.

### 12.3 Rolling adoption for users

- **Step A:** Upgrade Penelope to 0.5.0 while continuing to use old saga/task/timeline APIs. No history conversion.
- **Step B:** Adopt canonical `penelope::saga`/`penelope::task` imports and common inspection/diagnostics, with identical behavior.
- **Step C:** Introduce `penelope::machine` to a new or carefully isolated business-state domain; test its event transitions as a pure function.
- **Step D:** Opt into machine journal persistence and outbound intents where crash recovery is needed; configure scope/schema, quotas and authorization.
- **Step E:** Compose task/saga effects through the verified event bridge; adopt optional timeline for bounded speculative domains only.
- **Step F:** Migrate old imperative domain state only by writing explicit adapters and invariants; no blanket automatic conversion of arbitrary mutable structs into a safe state machine.

### 12.4 Rollback and reconciliation

Before production migration, maintain a backup of the 0.4 database and the exact old binary/source used for replay. A machine schema addition should not corrupt old data, but writing **new** machine events cannot be rolled back by simply reinstalling 0.4 binaries. If a new machine handler has already triggered external effects, recover via forward reconciliation or explicit compensating domain operations, never by deleting/replaying records as though effects had not occurred.

---

## 13. Verification plan, performance and maintenance criteria

### 13.1 Required test layers

**Unit / model checks**

- Flat machine legal/illegal transition table and terminal/unknown event behavior.
- HSM nesting, bubbling, override, guard rejection, LCA entry/exit order, internal transitions and bounded depth.
- State-local enum data construction and no accidental mutable publication bypass.
- `MemoryMachine` atomicity on faults, resource limits, panic/unwind and staged intent failure.
- Transition identity and provenance correctness independent of debug strings.

**Property-based / model-based**

- Compare a randomly generated bounded machine against an independent reference transition oracle.
- Accepted sequence replay yields the same state/revision and intent-plan identities as live evaluation, under pinned deterministic inputs.
- Duplicate identical event does not advance state; duplicate conflicting event never advances; stale revision never publishes.
- HSM refactor preserves flat-machine semantics when hierarchy has one active level.
- A history split into arbitrary pages reconstructs identically to a contiguous history, or fails on malformed boundaries.

**Fuzzing**

- Machine v1 wire parser/golden vectors and malformed schema/payload decoding.
- Dynamic event sequences, guard errors, nested state paths, counter overflow and capacity exhaustion.
- Replay under mutation of order, duplicate IDs, scope, fingerprint and event digest.
- Cross-component intent/result substitution and authorization receipt validation.
- SQLite serialized machines under repeated kill/reopen cycles with an independent model oracle.

**Concurrency and crash**

- Two or more writers racing the same head: exactly one distinct accepted transition; duplicates return exact matching receipts.
- Lost ACK after committed machine transition: reconstruct exact record and still dispatch each stable intent with consistent idempotency semantics.
- Crash after external delivery but before verification: outcome remains unknown until independently reconciled; never interpreted as safely repeatable by default.
- Simultaneous snapshot/replay readers never see a state paired with a different accepted head.
- Poisoned task supervisor and stopped generation cannot be bypassed through the generic `Machine` interface.

**Compatibility**

- Old serialized saga/task/timeline fixtures from v0.4.0 match exactly and still replay with the same policy decisions.
- External users compile unchanged against retained reexports and canonical new examples compile on Rust 1.85.0.
- Old SQLite process/task fixture DB loads under the new version; v0.4 code does not silently reinterpret machine tables.
- Existing StateChronicle signed-event conformance remains intact, including rejected forged or uncommitted evidence.

**Portable and architecture**

- `no_std` and no-alloc checks on machine core; RISC-V bare-metal build.
- `cargo metadata` dependency checks against exact allowlisted crate graph and external feature-matrix checks.
- Production core remains free of `std`, broker, database and runtime dependencies.
- One canonical public API; legacy ports remain discoverable without requiring downstream source renames.

### 13.2 Baseline performance methodology

Measure **same functional work** at v0.4 and v0.5. Do not compare a pure machine `match` against a durable SQLite transaction or declare a ratio across different guarantees. Keep the 0.4.0 historical measurements and their limitations intact, then record new same-machine data.

Recommended workload matrix:

| Workload | Profiles | Report |
| --- | --- | --- |
| Flat machine | 2/8/32 states, 2/8/32 events, guard accept/reject | p50/p95/p99 ns, allocations/op, throughput |
| Hierarchical dispatch | depths 1/2/4/8/16; handled at leaf/parent/root | latency by depth, bounded state-path workspace, allocs |
| State representation | ~8 B, ~256 B, ~4 KiB, ~64 KiB owned state | state copy/clone cost, user-computation split |
| Transition intent staging | 0/1/8/32 intents, quota failure | staging and publication overhead, allocations |
| Legacy linear/graph saga | Exact old benchmark fixtures incl. parallel variants | old vs adapted path, p50/p95, throughput, RSS |
| Legacy task | Poll, checkpoint, startup/restart, replay | overhead, unchanged results, memory |
| Timeline over machine | live input, correction horizons 1/8/32/128, state widths | correction/replay cost vs existing 0.4 timeline API |
| Durable machine SQLite | single writer, duplicate, 2/8/32 contenders, snapshot/no snapshot | commit/s, p95/p99, WAL size, disk writes, RSS |
| End-to-end | state transition + committed intent + task/saga result | separated commit/dispatch/effect cost and backpressure |

**Provisional gates** (approve in F0 after real baseline measurements):

- Core flat transition in the `no_std` profile introduces **zero internal heap allocations** for allocation-free consumer state and effect types.
- Legacy saga/task/timeline paths should show **no statistically meaningful regression** under identical workloads; a sustained >5% median degradation or >10% p95 regression is a review blocker until diagnosed and explicitly accepted with evidence. These are policy proposals, **not** results already measured.
- Machine HSM dispatch should show bounded scaling with active hierarchy depth; no unbounded scans over unrelated machine instances.
- Database throughput regressions must be evaluated against actual safety work/transaction contents, with WAL/page-cache environment documented. More safety checks are not automatically “free”; do not mask them by reducing tests or changing data.
- Track cold start/build cost, per-crate dependency count, binary size, peak RSS and user-facing boilerplate. A coherent refactor should not impose a heavy hosted dependency on pure machines.

### 13.3 Maintainability acceptance

The refactor must **remove** duplicated plumbing rather than merely add another layer:

- One authoritative public definition for machine transition outcomes, errors and candidate publication; specialized concepts map through documented adapters.
- Shared transition provenance and diagnostic view, rather than four independently named uncorrelated log conventions.
- Reused SQLite transaction/worker/receipt helpers without copying large pre-existing implementations.
- Single canonical user tutorial and examples that cover machine/saga/task/timeline integration.
- Documented rationale for any remaining duplicate-looking code: legacy wire contract, distinct recovery semantics, latency, or no-alloc boundary.
- Prefer focused tests of public invariants and independent models over duplicating dozens of near-identical tests of implementation details.
- Measure code and test maintenance before/after (source LoC, production duplication, dependency edges, test runtime, flaky failures), but **never impose a numeric line-deletion target** that incentivizes removing safety checks.

---

## 14. Release definition of done (non-negotiable)

Penelope 0.5.0 is **not complete** if only a `penelope-machine` crate is added, if sagas/tasks remain unrelated from the consumer's perspective, or if durability still requires consumers to invent their own identity/commit/replay protocols.

All these acceptance conditions must hold:

- [ ] **Unified programming model:** Flat and nested application state machines are first-class, with typed states/events, guards, explicit rejection, deterministic event handling, staged effects and observable provenance.
- [ ] **No mandatory extras:** A pure in-memory machine compiles/runs without a database, saga, task supervisor, timeline, async runtime, macro or heap requirement imposed by Penelope.
- [ ] **Real integration:** Existing saga and task implementations are accessible as specialized machine lifecycles through a coherent facade; their events/effects interact with application machines only through typed, verified admission.
- [ ] **No new rewind engine:** Optional timeline machine use reuses the real 0.4.0 timeline correction/finality implementation; old timeline guarantees remain intact.
- [ ] **Durability exists:** Machine-specific versioned event protocol, atomic head/CAS, duplicate/conflict handling, verified commit receipts, recovery and outbox intent admission have a qualified SQLite adapter.
- [ ] **No effect guessing:** Unknown external outcomes, crash ambiguity and reconciliation rules are enforced throughout composition.
- [ ] **Proven compatibility:** Frozen 0.4.0 saga/task/timeline wire fixtures and external API usage tests pass; existing deployments have an upgrade/rollback playbook.
- [ ] **One public API:** Canonical `penelope::*` namespaces and cohesive docs replace subsystem-by-subsystem onboarding, with valid old reexports and a migration table.
- [ ] **Refactor actually reduces fragmentation:** Common verified concepts and helpers have one implementation/contract, and any remaining duplicated-looking code is explicitly justified.
- [ ] **Portable and bounded:** New core meets current `no_std`/MSRV/architecture limits and demonstrates failure isolation and capacity bounds.
- [ ] **Qualification complete:** Existing CI, chaos/property/fuzz, conformance, migration, and performance gates pass on the exact source intended for release, with artifacts and scope documented.
- [ ] **Release controlled:** Version/changelog/docs/package order are accurate, and publication is separately authorized.

**If one major gate is incomplete:** Do not advertise a complete unified 0.5.0. Either finish the work or deliberately narrow the advertised release with a changelog that names the missing capability; do not silently reclassify a major safety feature as finished because an example happens to work.

---

## 15. Suggested PR/issue breakdown

The following units are intentionally reviewable. A PR can map to one phase or a narrow vertical slice within it.

| ID | Change | Depends on | Must prove |
| --- | --- | --- | --- |
| `PEN-050-00` | v0.4 baseline and external compatibility harness | — | Reproducible baseline and frozen fixtures |
| `PEN-050-01` | Unified lifecycle contracts + protocol specification | 00 | No ambiguous safety ownership |
| `PEN-050-02` | Portable `Machine`/transition/rejection/intent core | 01 | No-std, no-alloc profile, atomic candidate isolation |
| `PEN-050-03` | Hierarchical semantics and typed paths | 02 | Entry/exit/parent fallback truth-table tests |
| `PEN-050-04` | Canonical public facade and compatibility exports | 02 | External compile fixtures and no extra default deps |
| `PEN-050-05` | Graph/linear saga machine adapters and helper dedupe | 02,04 | Differential legacy parity incl. error/bytes |
| `PEN-050-06` | Task model adapter and secure effect bridge | 02,04 | v1 task replay/fencing/reconciliation parity |
| `PEN-050-07` | Timeline machine adapter and finality validation | 02,04 | Old timeline correction/effect finality proof |
| `PEN-050-08` | Machine protocol v1 golden codec and store ports | 01,02 | Strict model/wire/fuzz invariants |
| `PEN-050-09` | SQLite machine journal and atomic outbox | 08 | Kill/reopen/CAS/ambiguous-ACK model tests |
| `PEN-050-10` | Cross-component integration examples + diagnostics | 05–09 | No side-effect duplication and complete provenance |
| `PEN-050-11` | Refactor consolidation and maintenance audit | 05–10 | Reduced duplicated pathways, no weakened checks |
| `PEN-050-12` | Docs, migration guide, performance gates | 04–11 | Canonical developer path and parity measurements |
| `PEN-050-13` | Release candidate qualification | 00–12 | All section 14 gates on pinned SHA |

**Suggested review order:** Kernel semantics before macros; adapter correctness before deleting old helpers; SQLite durability after machine protocol is frozen; final facade/documentation cleanup after every subsystem is verifiably integrated. Avoid a single giant “rewrite Penelope” PR, which would obscure regression attribution.

---

## 16. Key decisions to record as ADRs

Record these decisions explicitly before code migration:

1. **ADR-050-01 — Machine contract:** Reference-based pure reducer returning owned state, or isolated scratch/transactional state? Document large-state performance and no-Clone behavior.
2. **ADR-050-02 — HSM model:** Typed nested path representation, exact event bubbling/guard precedence and entry/exit order.
3. **ADR-050-03 — Candidate/publication:** How `PreparedTransition` binds state, event, head and intents; whether it is consuming/opaque and panic-safe.
4. **ADR-050-04 — Protocol:** Machine v1 canonical encoding, stable IDs, schema/definition pinning, event digest and committed receipt binding.
5. **ADR-050-05 — Durability:** Head vs event log authority, local transaction scope, immutable payload retention, snapshot/recovery strategy.
6. **ADR-050-06 — Refactor limits:** Shared code extraction map vs preserved saga/task/timeline semantic types.
7. **ADR-050-07 — Hosted authorization:** New machine action scopes and compatibility with current `ProcessAuthorizer`, without modifying old exhaustive enums casually.
8. **ADR-050-08 — Feature/defaults:** Which modules/features the user sees by default, any optional macro, and mandatory checks for `no_std`.
9. **ADR-050-09 — Migration:** What constitutes schema-compatible restart, when an explicit transformation is required, and unsupported downgrade cases.
10. **ADR-050-10 — Performance policy:** Same-work baseline, tolerances, release exceptions and evidence retention.

---

## 17. Source index — inspected 0.4.0 implementation

All URLs below refer to the inspected `main` commit so line content does not silently drift after future changes. `v0.4.0` was released separately; this plan does not assert all code was freshly built or stress-tested during its drafting.

- [Repository at baseline SHA](https://github.com/STEXS-Technologies/penelope/tree/42ec187c9adfc7403b06215f405f2575f53c35c2)
- [Released v0.4.0](https://github.com/STEXS-Technologies/penelope/releases/tag/v0.4.0)
- [Workspace package topology](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/Cargo.toml)
- [README: saga, task and timeline scope](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/README.md)
- [Current public facade and reexports](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope/src/lib.rs)
- [Linear/graph definitions and graph bounds](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-executor/src/engine.rs)
- [Graph transitions, identity checks and ordered replay](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-executor/src/graph.rs)
- [Task contract, states, record, recovery modes](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-core/src/task/contracts.rs)
- [Task projection and validated transitions](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-core/src/task/projection.rs)
- [Task control/recovery implementation](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-core/src/task/control.rs)
- [Timeline model and snapshot/input contracts](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-timeline/src/contracts.rs)
- [Timeline scratch step and validation](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-timeline/src/step.rs)
- [Timeline live atomic publication](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-timeline/src/live.rs)
- [Timeline effect eligibility/durable handoff](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-timeline/src/handoff.rs)
- [Saga atomic process commit and ports](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-ports/src/implementation.rs)
- [SQLite process store](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-sqlite/src/process.rs)
- [SQLite adapter boundaries](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/crates/penelope-sqlite/src/lib.rs)
- [Release contract, tests and compatibility](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/docs/RELEASE.md)
- [0.4.0 qualification and explicit limits](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/docs/qualification-0.4.0.md)
- [Timeline implementation/API mapping](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/docs/timeline-foundation.md)
- [Layer/dependency architectural guard](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/scripts/check-layer-boundaries.sh)
- [CI verification jobs](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/.github/workflows/ci.yml)
- [Change history v0.4.0](https://github.com/STEXS-Technologies/penelope/blob/42ec187c9adfc7403b06215f405f2575f53c35c2/CHANGELOG.md)

---

## Final architectural position

**Penelope 0.5.0 should be presented as one library with one fundamental contract:** deterministic, typed state transitions with explicitly staged effects and optional durable execution.

A user starts with `Machine`. When transitions become external, multi-step and compensating, they compose `Saga`. When computation needs checkpointed restart and fencing, they compose `Task`. When accepted or speculative computation must support bounded correction, they opt into `Timeline`. All retain the same causal and diagnostic story, without pretending their records, trust sources or recovery policies are interchangeable.

**Success is not “we now have four features.” Success is that they behave like one deliberately designed system, with existing 0.4.0 guarantees preserved and less duplicated implementation to maintain.**
