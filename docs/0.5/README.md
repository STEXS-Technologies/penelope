# Penelope 0.5.0 implementation

Status: initial stabilization and facade consolidation; not release qualification
or an implemented machine API.

The [imported proposal](plan.md) is the source plan. Its baseline SHA matches
this checkout. Development remains on the existing branch, with versions at
0.4.0 until qualification. This directory separates planned behavior from the
current supported [release contract](../RELEASE.md).

The product goal is one framework for deterministic state evolution and reliable
execution. Users enter through `penelope`, learn one event/decision/publication/
effect lifecycle, and opt into saga coordination, task recovery, durability or
timeline correction. Internal crates serve dependency and deployment boundaries.

## Work order and completion gates

| Phase | Work | Status | Completion evidence |
| --- | --- | --- | --- |
| F0 | Freeze and reproduce 0.4 baseline | In progress | [Baseline](baseline.md), compatibility fixture, CI, chaos and comparable performance runs |
| F1 | Resolve kernel, hierarchy, publication, protocol and feature contracts | Design in progress | ADRs and compiling public API fixtures |
| F2 | Allocation-free machine kernel and hierarchy | Pending | Truth tables, failed staging/publication, replay oracle, portable build and allocation probe |
| F3 | Saga adapter and proven shared internals | Pending | Legacy/new differential decisions, errors and bytes |
| F4 | Task adapter and verified composition | Pending | Recovery, fencing, ambiguity and journal parity |
| F5 | Machine adapter to existing timeline | Pending | Correction, replay, scope, scratch and finality parity |
| F6 | Machine journal, SQLite and effect bridges | Pending | Independent codec, CAS, duplicates, crash/reopen, quotas and receipt verification |
| F7 | Consolidate API, internals, docs and examples | Facade paths established; integration pending | Canonical end-to-end example, migration fixtures, maintenance and performance comparisons |
| F8 | Qualify candidate | Pending | Exact-SHA evidence for every section 14 release gate |

Do not call a phase complete because its API names exist. In particular, namespace
aliases alone do not integrate the engines. Start production refactoring after F0
and settle kernel contracts before constructing durable protocols around them.

## Refactor acceptance policy

Each extraction or simplification records the duplicated responsibility, its
canonical owner, old paths removed, invariant tests retained, and equivalent-work
performance evidence where relevant. Prefer fewer decision paths and less shared
mutable state. Lines removed are supporting evidence, not a correctness metric.

Keep saga action correlation, task generations and recovery authority, timeline
revision/finality, and external canonical evidence distinct. Share helpers only
where semantics, error precedence and publication boundaries are equivalent.
Do not replace meaningful protocol types with generic integers or universal events.

Preserve the existing property, crash, wire, allocation, conformance and migration
gates. Unknown effect outcomes continue to require reconciliation. There is no
permission to weaken checks, widen bounds or rewrite old histories to simplify
implementation. Follow [compatibility requirements](compatibility-matrix.md).

## Decisions to settle before implementation

Use the proposal's ADR-050-01 through ADR-050-10 identifiers. Record concrete
choices and consequences for reducer ownership, hierarchy bubbling and hook
order, candidate identity/publication, independent machine wire encoding,
durable authority, extraction limits, authorization, features, migration and
performance tolerances. Proposal sketches remain sketches until compile-tested.

[Initial verification and baseline deviations](verification.md) record the
completed checks, approved MSRV change and remaining qualification work.
