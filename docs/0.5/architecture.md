# Unified state and execution: contract decisions

These decisions describe the implemented kernel, hierarchy, adapters and storage
boundaries. Compiling external consumers cover flat and hierarchical models,
prepared views and durable composition. Existing 0.4 protocols remain authoritative
for legacy engines; final release qualification is tracked in [verification](verification.md).

## ADR-050-01: reference-based reducer

The portable kernel takes `&State`, `&Event` and immutable supplied context, and
returns an owned candidate state. The trait has no `Clone`, `Default`, codec,
heap, scheduler or async requirement. Consumers with large states may construct
an isolated candidate with application-specific storage; aliasing mutable live
state is outside the supported profile. Rust cannot enforce consumer purity.

The output is accepted, rejected with a bounded typed reason, or unhandled.
Evaluation errors retain the model's concrete error type. Rejection and
unhandled do not increment revision or expose staged effects. Invariant
validation happens before publication and must not mutate the current state.

## ADR-050-02: hierarchy is a bounded helper

Use a single active path with stable model-owned node IDs and explicit parent
relations, supplied by the model. No reflection, registration map, parallel
regions or mandatory macros. Dispatch walks leaf to root with a caller-sized
maximum depth; repeated IDs and exhausted depth fail closed.

Unhandled alone permits parent fallback. Rejection, evaluation error or intent
staging failure terminates the whole attempt. Guard selection receives no intent sink, so an unselected handler cannot stage
provisional intents. A staging error remains fatal even if a handler ignores it.

External transitions exit leaf up to (excluding) the least common ancestor,
perform the transition action, and enter ancestor-to-leaf. An external self-edge
exits and reenters the leaf. The active source leaf determines the exit path even
when selection bubbles to a parent; selection does not introduce a separate
SCXML transition domain. An internal transition retains the leaf and runs its action without entry
or exit. No implicit completion-transition loop. All hooks return candidates or
stage intents, with the entire attempt subject to one publication boundary.

## ADR-050-03: opaque, consuming candidate and coherent publication

Preparation binds exact scope, pinned definition/schema, predecessor revision,
event identity and the complete candidate/intent batch. Prepared fields are
private. A candidate cannot be constructed from arbitrary payloads, cloned or
published twice. An in-memory candidate is tied to the owner it was prepared
against; two independent owners must not accept each other's candidates merely
because their external scope/revision values happen to match.

Prefer a borrow-bound prepared handle or immediate `handle` operation for local
publication rather than inventing process-global owner IDs or randomness in the
portable core. Durable candidates instead bind the store's exact scope/head and
commit identity; they require independent verified receipts.

Preflight every bound and counter before changing published data. Publish state,
revision, event provenance and intents together, using moves only. Release old
state and effects after the whole tuple is coherent: consumer destructors can
panic. A panic during cleanup after publication is not evidence that the event
was uncommitted; recovery/inspection must observe the coherent published tuple.
No mutable live-state accessor is provided.

## ADR-050-04 and 05: independent durable protocol and authority

New application machines require `penelope.machine.v1`, bounded canonical
encoding and a machine-specific journal. Process `AtomicProcessCommit` is not a
generic machine transaction. The initial accepted-batch [metadata layout and
independent goldens](machine-wire.md), canonical payload preparation and
[scoped journal/SQLite snapshot profile](machine-storage.md) are implemented.
Source-authorized leased delivery and native task/saga admission are implemented.
Both bridges retain native execution and recovery authority; complete release
qualification remains open.

Persist immutable accepted event bytes, pinned model/schema, exact predecessor
and next head/state, and the entire ordered intent batch in one local transaction.
Exact duplicate event/content returns the same verifiable receipt; same ID with
different content conflicts. An absent ACK is unknown until authoritative lookup.
A content digest identifies bytes and does not authenticate a rewritten store.
No claim of cross-store atomicity or persistent timeline correction follows.
The initial recovery profile retains a complete state snapshot per accepted event;
restoration does not execute events. History is append-only within explicit limits
and refuses further admission rather than discarding deduplication evidence.
SQLite schema 7 adds separate journal tables; schema 8 adds bounded delivery
metadata without rewriting accepted event bytes.

The task receiver binds its policy identity and the complete machine source to
a native task scope. It uses the existing task projection/journal and records only
the initial intent. Receipt verification reads the exact native intent and input;
source acknowledgement remains a separate fenced transaction. Native supervisor
recovery and reconciliation rules retain all execution authority. The async
SQLite task adapter moves that same journal onto the existing worker rather than
duplicating its evaluator, cache or SQL. See the
[handoff contract](machine-storage.md#admit-a-recoverable-task).

## ADR-050-06: extraction boundaries

The candidate/publication pattern is shared vocabulary, not evidence that every
engine can use one implementation. The existing timeline staging buffer includes
step/branch provenance and merged correction quotas. Retain those contracts.
Extract a bounded admission helper only with differential checks for error
precedence, poisoning (including caught destructor panic), ordering and bounds.

Task control keeps journal-before-launch, generation fencing and poison-until-
reconstruction. Saga keeps action/result coordinates, compensation order and
manual review. Their adapters present truthful views of existing history,
including fields the legacy protocol did not record. No synthetic rewritten
machine histories or unchecked identity conversions.

The consolidation audit tracks these authoritative owners:

| Responsibility | Shared owner and callers | Boundary retained |
| --- | --- | --- |
| Prepared lifecycle inspection | Core `PreparedDecision`: machine, saga and task views | Native state/events remain typed; inspection confers no commit authority |
| Intent count, identity and byte preflight | `core::__private::admit_intent`: machine staging/wire and timeline effects | Payload ownership, poisoning and public error precedence stay local |
| Bounded wire cursor movement | `core::__private::{take_bytes, put_bytes}`: machine and timeline codecs | Distinct protocol tags, schemas, variants and error types |
| Blocking async I/O and response delivery | SQLite `DatabaseWorker<State>`: process, machine and native task adapters | Task journal owns its original connection, cache and transactions |
| Native committed process proof | SQLite `committed_receipt`/`verify_committed_batch`: idempotent commit and read-only verifier | Verification never retries writes or dispatches actions |
| Full source-to-receiver identity | `MachineIntentSource::binding_identity`: task and saga receivers | Host-pinned task/saga policies and native identities remain distinct |

Separate machine/task/process transactions remain necessary: each authenticates
different history and scope, and only its native owner may advance that history.
The machine outbox and process action leases also protect different authorities;
sharing a universal lease would erase source acceptance versus execution rights.
Thin protocol readers and error adapters are retained to preserve old wire/error
contracts and the allocation-free core boundary. The
[measured maintenance inventory](evidence/maintenance/manifest.json) records size,
dependencies, build/test cost and the limits of these consolidation claims.

## ADR-050-07 and 08: authorization and features

Pure reducers do not authenticate callers. Hosted admission verifies the trusted
principal and exact requested operation. Machine operations use independent
types instead of adding variants to legacy exhaustive authorization enums.
`MachineAuthorizer` now carries a host-owned associated context; `MachineStore`
authorizes every read, write and duplicate lookup before scoped information is
queried. Stored bytes and receipt DTOs never authenticate their own source.

Core machine support belongs in `penelope-core::machine`, without new packages
or feature dependencies. Keep facade defaults unchanged. Timeline, SQLite,
protected workers and any future macros remain opt-in composition. The existing
facade is hosted; allocation-free/no_std qualification targets the portable core,
not the complete umbrella dependency tree.

## ADR-050-09 and 10: migration and performance

Keep old saga/task/timeline decoders and histories. Pin model/schema at instance
creation; changing interpretation requires an explicit authenticated migration.
Unsupported downgrade is documented rather than guessed.

Use equivalent baseline/candidate workloads with the same toolchain, capacities,
features and payloads. Preserve raw runs, source/binary identity, allocations,
latency distributions and supported profile limits. Set workload-specific
regression tolerances from measured baseline variance before accepting a hot-path
refactor; no invented universal overhead percentage or timing from loaded test
runs. Final release qualification remains a separate exact-revision gate.

## Approved MSRV amendment (2026-11-15)

The user approved raising the workspace minimum from Rust 1.85 to Rust 1.89
to adopt the patched bundled SQLite dependency. Every published crate inherits
the workspace minimum, and CI tests it explicitly. The development toolchain
remains independently pinned. Original baseline evidence remains scoped to the
old dependencies and Rust 1.85; do not rewrite it as new-version evidence.

The first [shared admission extraction](staging-extraction.md) has two real
callers and preserves domain-specific poisoning, provenance and error meanings.
Its cross-crate helper is internal support, not a new application API.

## Prepared decision inspection

One portable `PreparedDecision` trait supplies read-only before-state, candidate,
native provenance and native intents. It intentionally supplies no generic commit,
authorization or dispatch operation. Saga adapters use the original decision
engines; task preparation shares the existing projection evaluator. Missing old
coordinates remain missing rather than receiving synthetic machine identities.
See [public APIs](public-api.md) and [adapter evidence](prepared-adapters.md).
