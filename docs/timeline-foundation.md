# Timeline foundation (0.4.0 development)

Baseline: released `v0.3.0`, commit
`788c282e359477469cd805c7ea82f2da22f85a87`. The release workflow verified
and published that revision successfully. This document records the actual API
mapping and the implemented foundation; it is not a release announcement.

## Mapping the released foundation

| Requirement | Existing 0.3.0 API / evidence | Timeline boundary |
| --- | --- | --- |
| Portable bounded supervision | `penelope_core::task::control::{TaskControl, TaskWorkspace}` | Timeline metadata uses a separate module; no change to task recovery |
| Typed scope, clock and generation | Task identity newtypes, `TaskTick`, `TaskGeneration` | New timeline identity, logical step, branch and revision types; no implicit conversions |
| Saga/task wire compatibility | Domain `golden_wire` and executor `task_contracts` fixture tests | Existing fixtures and serialized protocols remain unchanged |
| Durable process handoff | `penelope_ports::AtomicProcessCommit`, SQLite process adapter | Future finalized-effect bridge; not yet implemented for timelines |
| Verified task result and effect evidence | Executor composition, verified-result and effect-bridge tests | Does not authorize speculative dispatch or reverse issued effects |
| Optional production infrastructure | Dedicated SQLite and protected Linux runtime crates | No new infrastructure dependency in portable core |
| Crash and fencing validation | Existing adapter/process suites and inherited CI | Gates retained; timeline concurrency testing will supplement them |

## Implemented metadata planning

`penelope_core::timeline` provides distinct fixed-width identities, a validated
head, complete post-step anchor descriptors, immutable input descriptors and a
borrowed `ReplayPlan`. It owns no application state, payload storage, heap,
clock, scheduler, journal or runtime. Timeline metadata currently has **no wire
format or Serde implementation**; a versioned record protocol and golden fixtures
are still required before persisted timeline records are supported.

`S_t` means state after input `I_t`; step zero is the genesis anchor. A state
correction at `t` consumes precisely `I_(t+1)..I_head`. An input correction at `k`
requires the anchor `S_(k-1)` and replaces only the first original input in the
supplied contiguous range. Its stable input identity must match and its input
revision must increase. Plans borrow the immutable original input slice; they
neither rewrite the original history nor synthesize absent payloads.

Preparation checks the exact expected revision, restore scope/model/schema and
branch, correction range, contiguous input order, replacement identity/version,
capacity and revision exhaustion. `check_head` compares the complete pinned head
before any later publication. It is a preflight check, **not** an atomic CAS or
proof that application replay ran successfully.

Finality is `Option<TimelineStep>`: `None` means no prefix has been effect-finalized.
A state replacement at or before a finalized step is refused; replacement input
at or before it is also refused. Restoring the unchanged finalized predecessor
anchor to replace a later input remains possible. Authority observations do not
advance finality implicitly.

Consumers must authenticate corrections, validate authority ordering, establish
anchor lineage and snapshot completeness, pin immutable payloads to their
descriptors and retain the whole consistency domain. This first API admits only
anchors and resolved input descriptors on the current branch. It does not yet
resolve ancestor branches or prove trust from a digest.

## Verification and remaining implementation

Exhaustive short integer histories check state and input correction indexing
against direct reference sums. Adversarial cases cover gaps, duplicates, order,
scope/model/schema/branch substitution, stale heads, finality conflicts, quotas
and counter exhaustion. Newtype constructor/arithmetic checks are registered in
the existing fuzz inventory. RISC-V checks keep the module allocation-free and
`no_std`; no performance percentage is claimed.

Remaining work includes immutable branch reservation/lineage and transition
projection; snapshot and payload stores; resumable scratch replay; candidate
validation; atomic state/head promotion and competing-correction tests; effect
intent staging, authority/finality admission and durable 0.3 handoff; bounded
retention; independently versioned timeline records; the optional consumer crate
and facade feature; simulation and non-game samples; complete fuzz/model and
performance qualification. None of those paths is implied by a successful
metadata plan. Workspace release versions remain 0.3.0 until the new consumer
crate and its package graph are introduced.

Foundation validation uses the existing golden-wire, task-contract, adversarial
and architecture suites, stable/Rust 1.85 core tests and doctests, strict Clippy
and rustdoc, bare-metal RISC-V compilation, and layer checks including negative
controls. A 10,000-run fixed-seed newtype smoke test passes; that run is not
coverage-instrumented and does not replace the future timeline fuzz campaigns.
