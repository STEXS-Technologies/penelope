# penelope-timeline

Runtime-neutral rewindable computation under development for Penelope 0.4.0.
The default build is allocation-free `no_std`. Enable the facade's optional
`timeline` feature to re-export this crate; existing facade users do not acquire
this dependency by default. The optional `std` feature supplies `SharedTimeline`,
which places the entire live publication under one shared lock.

A model implements `RewindableComputation`; snapshot and input adapters supply
complete isolated state and exact immutable payload versions. `ReplayPlan` pins
a whole consistency domain and input range. `ReplaySession` restores scratch,
executes at most the supplied step budget, stages bounded effects and yields.
`CandidateValidator` checks consumer invariants or same-baseline state digests.
Only successful consuming validation produces a `ReadyCandidate`.

For the owned-state convenience profile, `LiveTimeline::apply_input` derives the
next record and computes, validates and publishes from an isolated `Clone` of
live state. `begin_correction` returns a scoped handle: call bounded `advance`,
then `publish`, or drop it to cancel. The reservation is released automatically
on errors and unwind. Closures implement `CandidateValidator`; `CapturedInput`
and `CapturedSnapshot` provide small owned adapters. Captured-input arrays/slices
use a checked direct slot hint for contiguous step buffers and an exact-record
scan otherwise; both paths recheck payload integrity. Pass slices directly to
scoped/low-level replay, pinned sessions and live-step preparation. Run
`cargo run -p penelope-timeline --example quickstart --locked` in the workspace.
These helpers do not transact history admission or authenticate authority;
`FixedHistory` lineage/retention remains explicit. The scoped handle exclusively
borrows the live owner. Lower-level sessions support competing publications and
adapters without a mandatory state `Clone`.

`LiveTimeline` publishes a validated state/head/intents tuple under exclusive
ownership and checks the exact pinned revision. Abandoned branch reservations
remain consumed. Concurrent candidates must use the same owner/lock; the first
successful publication supersedes the others. New live steps use the same model
through `prepare_live_step`, and their publication also supersedes stale replay.
Consumer destructors run only after the new publication is coherent.

Models must not read uncaptured time/randomness or perform irreversible I/O.
They may only stage `EffectSink` intents. Authority observations and irreversible
effect finality are separate. Only an explicitly finalized prefix produces
`EligibleEffect` values; `EffectHandoff` must deduplicate their scoped identity,
verify payload bindings and reconstruct authoritative commits after ACK loss.
A handoff receipt proves durable admission, not external execution or delivery.
Handoff orders eligible intents by logical step and stable identity. Diagnostic
intent iteration has unspecified storage order. Fixed buffers keep a dense
occupied prefix; bulk handoff sorts metadata without heap allocation.

The consumer owns correction authentication, authority trust, snapshot completeness,
lineage admission, input payload integrity, state-isolation mechanics and finality
proofs. Hashes do not establish authority. This is computation replay, not rollback
of effects already issued. Sharing interior-mutable aliases to live state violates
the publication contract. A poisoned hosted write lock requires reconstruction.

`FixedHistory` offers fixed-capacity immutable inputs and full snapshots, explicit
byte limits, ancestry resolution and safe floor eviction. `PinnedReplaySession`
borrows history across yields, preventing eviction while replay still depends on
it. Corrections remain imports until selected on a child branch. Capture checks
capacity before cloning; consumers can supply other snapshot/input adapters.
Input reads try a checked step-relative slot hint before an exact scan; complete
descriptor, content and declared-charge checks remain required. Imports, eviction
and branch aliases can use the fallback without changing lookup semantics.
Canonical input resolution follows inherited lineage without scanning child slots
at or before the fork point; child appends and materialized roots use local lookup.
Snapshot lookup filters by logical step before full exact matching and verifies
a selected canonical entry directly, avoiding a duplicate scan.
If a payload destructor unwinds during leaf retirement, the leaf remains
discoverable and cleanup can be retried; removed payloads remain uncharged.

`materialize_branch` copies a selected branch into independent bounded history,
preserving consumed branch IDs so old ancestry can be reclaimed after pins and
other candidates retire.

The independent `penelope.timeline.v1` codec serializes metadata into caller-owned
bytes; it does not establish a complete durable recovery image. The integer
simulation and SQLite optimistic-invoice examples demonstrate independent domains
using the same API, including a finalized-effect bridge with two lost-ACK/reopen
checks. See `docs/timeline-examples.md` in the repository for commands and contracts.
Release qualification remains in progress; the in-memory API does not establish
persistent timeline state or arbitrary-state determinism.
