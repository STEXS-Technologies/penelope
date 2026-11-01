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
clock, scheduler, journal or runtime. Its allocation-free binary record codec uses
an independent `penelope.timeline.v1` tag and strict fixed-width parsing, protected
by golden fixtures. See [the exact format and recovery boundary](timeline-wire-v1.md).
The codec does not by itself provide persistent state publication or authentication.

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
descriptors and retain the whole consistency domain. The core API admits
anchors and resolved input descriptors on the current branch. The optional history
adapter resolves ancestor aliases before planning; no digest proves trust.

## Verification and remaining implementation

Exhaustive short integer histories check state and input correction indexing
against direct reference sums. Adversarial cases cover gaps, duplicates, order,
scope/model/schema/branch substitution, stale heads, finality conflicts, quotas
and counter exhaustion. Newtype constructor/arithmetic checks are registered in
the existing fuzz inventory. RISC-V checks keep the module allocation-free and
`no_std`; no performance percentage is claimed.

The implementation includes a bounded metadata projection with non-reused branch
reservations, authority epoch/sequence ordering and independently advancing effect
finality. The new optional `penelope-timeline` crate owns isolated scratch replay,
bounded staged intents and consuming candidate validation. `LiveTimeline` publishes
state/head/intents under exclusive ownership; optional `SharedTimeline` uses one
hosted lock. Live steps run on scratch before publication and invalidate pinned
replays. Finalized intents cross an explicit idempotent durable-admission port;
ACK uncertainty retains the intent. This generic port is not yet the concrete
0.3 SQLite integration example.

Current tests cover incremental/zero-budget replay, restore/input/model/validation
failures, ignored effect exhaustion, caught model panics, competing candidates and
readers, aborted-branch ABA, new live input during replay, authority/finality
separation, lost handoff ACKs, mismatched receipts and destructor panic after a
coherent publication. These tests do not complete the release qualification.

`FixedHistory` provides fixed-capacity input versions, full snapshots and immutable
branch lineage. Imported corrections do not overwrite canonical parent facts or
become live snapshots. Children inherit inputs and only the unchanged state prefix;
corrected inputs and state anchors remain explicitly bound. Payload content and
charges are checked on admission and read. Capture checks slot and byte quotas
before cloning; the base model API still requires no `Clone`.

`PinnedReplaySession` holds an immutable history borrow across yields. Safe Rust
prevents mutation or eviction while pinned. Eviction requires a complete floor
anchor for every retained branch; branch retirement rejects live and depended-on
ancestors. There is no implicit eviction on capacity exhaustion. This conservative
profile buffers new inputs outside the store while replay is pinned.
`materialize_branch` copies a selected branch into an independent bounded store
with complete available anchors and resolved input versions. It preserves consumed
branch IDs, checks all charges before cloning and leaves the source untouched.
Owners can replace history after readers release it and other candidates retire,
reclaiming old ancestry without silently invalidating a retained branch.

Remaining work includes independently versioned timeline records and fixtures; concrete durable handoff,
simulation and non-game samples; comprehensive model/fuzz/performance campaigns;
and all inherited release/package gates. The workspace and path-dependency
versions now target 0.4.0; no 0.4 tag or publication has been requested.

Foundation validation uses the existing golden-wire, task-contract, adversarial
and architecture suites, stable/Rust 1.85 core tests and doctests, strict Clippy
and rustdoc, bare-metal RISC-V compilation, and layer checks including negative
controls. A 10,000-run fixed-seed newtype smoke test passes; that run is not
coverage-instrumented and does not replace the future timeline fuzz campaigns.
The timeline wire target separately passes 10,000 coverage-instrumented runs with
seed 401, including mutations of all seven golden variants. Full timeline
state-machine/effect fuzz campaigns remain outstanding.
