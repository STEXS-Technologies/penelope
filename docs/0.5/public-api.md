# Public API

This maps the implemented APIs and their authority boundaries. Final release
qualification remains open.
Use Rust 1.89 or later. The facade defaults and existing imports remain supported.

Choose the guarantees required by the application:

| Layer | What it provides | Application responsibility |
| --- | --- | --- |
| Rust types | Associated state/event/intent types, enum-owned state-local data and exhaustive matches; an exclusive prepared handle is consumed on local publication | Encode domain distinctions in the model. Types alone do not prove legal transitions or correct predicates |
| Runtime validation | Guards, candidate invariants, checked hierarchy paths, one selected transition and bounded intent staging before publication | Keep callbacks deterministic and free of external effects or mutable aliases into published state. Configured capacities do not bound arbitrary callback work |
| Durable execution | With a configured journal: event identity, transactional revision CAS, exact receipts, checked recovery without dispatch, and outbox/native admission protocols | Authenticate callers and stored evidence, pin the model/codec, configure durability and reconcile unknown external outcomes through the native task/saga protocol |

Local publication does not acquire durable guarantees. Enum state-local data
prevents attaching fields to the wrong variant; it does not make every invalid
domain transition unrepresentable. There is no general workflow typestate API.

| Entry point | Current role | Example or check |
| --- | --- | --- |
| `penelope::machine` | Pure model, bounded owner, consuming local publication, hierarchy | `first-machine` tutorial consumer, `counter_machine`, `nested_order` |
| `penelope::saga` | Existing linear/graph engines and prepared decision adapters | `trade`, `prepared_decisions` (linear and graph), `ci/saga-parity` |
| `penelope::task` | Task record/projection/control contracts, prepared record inspection, hosted supervisor | `prepared_decisions`, existing recovery examples |
| `penelope::diagnostics::PreparedDecision` | Shared read-only candidate, predecessor, provenance and intent inspection | Shared example inspection helper and independent facade consumers |
| `penelope::storage` | Process/task ports; machine codec, journal and authorization contracts | Durable examples, `prepared_decisions` |
| `penelope::runtime` | Existing runtime integration boundaries | Existing task/runtime examples |
| `penelope::integration` | Correlation with independently verified external evidence | Pinned signed-source conformance checks the adapter |
| `penelope::timeline` (feature `timeline`) | Captured `MachineTimeline` adapter over existing replay, history, finality and handoff | `timeline_machine`, independent facade consumer |

`PreparedDecision` is defined once in the portable machine module and reexported
by diagnostics. Its associated types preserve each engine's native state,
provenance and intents. It is not a generic commit/dispatch trait. Application
implementations of this inspection trait are likewise not admission evidence.

For diagnostics, read the native evidence at the stage being inspected:

| Question | Source and limit |
| --- | --- |
| What was evaluated? | `PreparedDecision::{before, candidate, provenance, intents}`; machine provenance retains supplied scope, event identity/digest, logical time and selected transition. These are proposed coordinates |
| Why was a local event refused? | After refusal, read the unchanged source state alongside the attempted event and `Rejected { reason }` or `Unhandled`. Refusals do not advance the head or create an accepted journal record; `nested_order` demonstrates this |
| What committed? | Authenticated `MachineCommitReceipt` plus the exact prepared/stored batch; disposition distinguishes new commit from exact duplicate. A lost acknowledgement remains unknown until lookup/retry resolves it |
| What caused it? | The committed header's optional typed `causation`; absence remains absence |
| Was an intent admitted downstream? | Authorized outbox status and verified native receiver receipt; acknowledgement establishes receiver admission |
| Did execution succeed? | The native task/saga result and its verified effect evidence. Claim, admission and source acknowledgement alone do not establish external completion |

State paths come from the typed model/state; they are not invented from a digest.
Legacy protocols retain their own provenance and explicitly missing fields.
Operational logs may redact payloads, but they are never the durable authority.

Start with `MemoryMachine::handle` for immediate local preparation, validation
and publication. `machine::Digest` reexports the shared SHA-256 content identity;
existing `task::Digest` imports name that same type. Local hashing authenticates
neither the caller nor an external result.

`MemoryMachine::prepare` returns an exclusive consuming handle; dropping aborts,
and `publish` makes the local state/head/intent tuple visible. Its view contains
only newly staged intents, while earlier committed pending intents stay with the
owner. Local publication supplies no durable admission or historical deduplication.

`MachineCommitRecord<INTENTS>` supplies the bounded
[`penelope.machine.v1` metadata codec](machine-wire.md). It binds event/state
digests, native provenance, causation and ordered intent metadata. Constructing
or decoding a record proves neither acceptance nor durable admission.
[Canonical payload preparation](machine-preparation.md) binds the accepted
candidate and checks exact receipt coordinates. `MachineStoredCommit` checks
retained payloads. `restore` validates a scoped snapshot without evaluating events;
`replay` consumes an isolated recovery owner, reevaluates one original event and
compares the complete regenerated batch before returning the owner. Both leave
durable delivery to the journal/outbox and do not dispatch effects. See the
[recovery profiles](machine-storage.md#storage-recovery-and-bounds).

The [optional SQLite journal](machine-storage.md) provides authorized admission,
coherent snapshots, exact lookup and bounded history. Its source-bound outbox
hands work to [native task](machine-storage.md#admit-a-recoverable-task) and
[saga receivers](machine-storage.md#admit-a-saga). Each receiver verifies and
acknowledges its own durable admission through the existing native protocol.

`prepare_linear` delegates exactly once to the original `decide` and borrows its
source input and definition. `into_parts` produces the original replay event and
`SagaDecision` together. `prepare_graph_start` and `prepare_graph_result` delegate
to the corresponding original graph operations; `into_decision` preserves their
original event, projection and action. Graph start owns its decision and does
not retain the definition; graph result borrows its predecessor for inspection.
There is no new saga interpreter or state owner. Persist and verify the existing atomic process commit before dispatch,
and route timer actions through their existing timer schedule path.

Linear source tenant/process are supplied by the caller; the legacy linear
projection did not record those coordinates. Graph scope remains in its native
projection. Neither adapter invents a durable sequence, machine revision, stable
machine transition ID, event digest or verified receipt that its old protocol did
not supply. Legacy input/projection authenticity and authorization boundaries
remain the application's responsibility.

`TaskProjection::prepare(record)` validates one isolated candidate through the
same private evaluator used by `propose` and `apply`. `PreparedTask::into_record`
returns the unchanged proposed v1 record. The projection view stages no runtime
intents: journal-first launch, generation fencing, unknown effect reconciliation
and runtime commands remain under `TaskControl` and the hosted supervisor. There
is no generic `publish` method on this view. It cannot manufacture a verified
journal receipt or start work. Applying a record still requires the existing
application/journal authority boundary.

The flat example, saga and task example share one generic inspection helper.
`ci/machine-consumer` compiles those exact example sources through the facade alone.
`ci/saga-parity` compares current legacy and prepared consumers against actual
0.4 observations. Task parity uses an exact frozen 0.4 projection implementation,
including its private recovery fields and original error precedence.

The [optional machine/timeline composition](timeline-machines.md) uses the same
counter model as the flat example. Captured context and native revisions remain
distinct from timeline branch/revision, and intents retain timeline finality.

The [canonical tutorial](../getting-started.md) composes durable machine
recovery, native task admission, verified worker results and saga admission.
Those bridges retain their native authority boundaries; prepared inspection
alone never supplies authority. See [verification](verification.md) for current
passing checks and open release gates.
