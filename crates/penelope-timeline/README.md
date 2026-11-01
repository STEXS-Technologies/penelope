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

`materialize_branch` copies a selected branch into independent bounded history,
preserving consumed branch IDs so old ancestry can be reclaimed after pins and
other candidates retire.

Versioned timeline records, the concrete durable bridge example and release qualification are still being implemented. Do not infer
persistence or arbitrary-state determinism from the current in-memory API.
