# Penelope 0.5 preparation

The [imported plan](plan.md) defines the scope and acceptance criteria. The machine
kernel, hierarchy, native task/saga bridges, SQLite outbox and timeline adapter
are implemented. **Final release qualification remains open.** Workspace versions
remain 0.4.0 during development; this branch is not a qualified 0.5 release.

Users start with the [canonical tutorial](../getting-started.md), then the
[public API map](public-api.md). Internal crates provide dependency and deployment
boundaries behind the facade.

## Plan status

| Phase | Current state | Evidence and remaining work |
| --- | --- | --- |
| F0 — Baseline | Frozen original sources, dependencies, wire/API observations and populated SQLite database | [Baseline](baseline.md), [compatibility](compatibility-matrix.md); equivalent-work performance review remains open |
| F1 — Contracts | Ownership, hierarchy, publication, wire, authority and feature decisions implemented | [ADRs](architecture.md), external consumer fixtures; final qualification remains open |
| F2 — Portable kernel | Flat and nested machines, bounded consuming publication | [Machine contract](machine.md), allocation/oracle/fuzz checks and dispatch measurements; full campaign qualification remains open |
| F3 — Saga | Native decision adapters and source-bound saga admission | [Public APIs](public-api.md), frozen legacy parity and [storage](machine-storage.md#admit-a-saga); final qualification remains open |
| F4 — Task | Native prepared views, task admission, worker recovery and verified result progression | [Canonical tutorial](../getting-started.md), [storage](machine-storage.md#admit-a-recoverable-task); final qualification remains open |
| F5 — Timeline | Machine adapter reuses existing correction/finality engine | [Composition](timeline-machines.md); retained legacy performance findings require review |
| F6 — Durability | Authorized SQLite journal, snapshots, outbox and native receivers | [Storage](machine-storage.md), [wire](machine-wire.md), [upgrade/rollback](compatibility-matrix.md); final qualification remains open |
| F7 — Coherence and maintenance | Canonical facade/tutorial, shared helpers, docs cleanup and maintenance inventory | [Extraction boundaries](architecture.md#adr-050-06-extraction-boundaries), [verification](verification.md); performance matrix and final audit remain open |
| F8 — Candidate | Pending | Exact-candidate CI, chaos, coverage, performance conclusions, versions/changelog and release evidence |

## Acceptance policy

Share concepts and helpers where semantics, error precedence and publication
boundaries match. Keep native saga correlation, task generations, timeline
finality and external evidence distinct. [Remaining separate implementations](architecture.md#adr-050-06-extraction-boundaries)
have explicit authority or protocol reasons. The measured maintenance inventory
does not claim an overall reduction in production source size.

Retain wire, recovery, crash, allocation, conformance and migration guarantees.
Unknown effects require reconciliation. Do not weaken checks, widen bounds or
rewrite histories to simplify implementation. The approved MSRV is Rust 1.89;
the development toolchain is independently pinned in `rust-toolchain.toml`.

[Verification](verification.md) records revision-scoped results and open gates.
Publication and tagging require separate authorization after qualification.
