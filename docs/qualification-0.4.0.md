# 0.4.0 branch qualification

The implementation starts from released `v0.3.0`,
`788c282e359477469cd805c7ea82f2da22f85a87`. This audit covers the rewindable
timeline branch, not authorization to merge, tag or publish a release.
The repeated full chaos campaign completed successfully with terminal exit 0.
The local qualification gates are complete. Each evidence archive retains its
own source revision and profile limits.

## Current verification scope

The latest runtime change, `735f809`, is covered by the
[materialization audit](#detached-materialization-audit). The subsequent
`aecb1d5` expands benchmarks and documentation without changing library production
source; all [seven CI jobs on that revision](https://github.com/STEXS-Technologies/penelope/actions/runs/41357643326)
pass, including stable and Rust 1.85 workspace checks. The
[state-width follow-up](#snapshot-state-width-qualification) records 640 measured
profiles and their limits.

The repeated 66-batch chaos campaign below retains its earlier production scope.
Later changes have their own targeted qualification; that campaign was not rerun
against every subsequent revision. Branch qualification does not imply a release
or establish correctness of arbitrary consumer integrations.

## Implementation and contract audit

| Area | Current implementation and evidence |
| --- | --- |
| Released foundation | [Actual baseline mapping](timeline-foundation.md#mapping-the-released-foundation); existing task/saga implementation files and fixtures are unchanged from the released tag |
| API refinement | [Concrete public API mapping](timeline-foundation.md#public-api-refinement); separate core planning, runtime execution, consuming validation and owned publication |
| Convenience API | Owned-state input application, scoped correction cleanup, closure validators and captured adapters; [quickstart and storage limits](timeline-examples.md#quickstart). Repeated single-slot failures/unwinds and the correction property/fuzzer exercise the convenience lifecycle |
| Independent identity and protocol | Core timeline identities/head/projection use fixed-width typed counters and scoped model/schema identity; seven independently assembled `penelope.timeline.v1` goldens and strict decoder tests |
| State/input indexing | `timeline_plans`, `history` and the shared model verify post-step state corrections and exact input predecessors; sparse-anchor and zero-input genesis cases exercise the runtime store |
| Immutable lineage and anti-ABA | `FixedHistory` imports, forks and parent aliases preserve original records; projection restore retains the branch high-water mark; retire/materialize tests refuse reuse and depended-on ancestors |
| Failure isolation | Scratch replay and live preparation preserve the accepted tuple on restore, input, model, validation, quota and stale-head failures; sink rejection poisons ignored failures; caught panics cannot validate incomplete replay |
| Atomic visibility | `LiveTimeline` owns state/head/intents; `SharedTimeline` locks that whole owner. Competing-candidate/readers and destructor-panic tests verify coherent publication under supported profiles |
| Consumer determinism | Same-baseline oracle/property tests and digest validation; explicit hidden-dependency, incomplete-snapshot and floating-point-order adversarial models. Arbitrary computation correctness remains a consumer obligation |
| Authority and effect finality | Independently ordered observations and finalized prefix; neither accepted observation nor promotion implicitly dispatches. New authority epochs cannot reverse finalized effects |
| Durable handoff | Eligible intents require exact durable admission receipts; optimistic invoice sample verifies atomic inbox/outcome/outbox rollback, wrong bindings, lost admission ACK and lost external delivery ACK with reopen/dedup |
| Bounded resources | Fixed slots and byte charges, bounded per-call replay, independent session/branch/intent limits, pre-clone capacity checks, and out-of-window refusal. Compile-fail pin test prevents eviction during retained reads |
| Generic consumer acceptance | Independent integer world and invoice domain types use the same API; [examples and downstream responsibilities](timeline-examples.md) cover the application boundary |
| Portable layering | Default core/timeline are `no_std`; bare-metal default/Serde/core and default/alloc/timeline checks pass. Layer tests include rejected dependency mutations. Default facade does not enable its optional timeline dependency |
| Performance | [Same-model ring comparison](timeline-performance.md), per-path percentiles, separate zero-allocation accounting, capacity matrix, batched core/full throughput and sampled hosted stack with a positive control |

Full snapshots are the supported convenience-store baseline. An allocating ring,
COW/delta store and persistent timeline transaction adapter are not supplied.
The storage traits permit consumer alternatives; optional COW/delta optimization
has not been justified or measured. Consequently there are no implicit delta-base
dependencies or unsupported claims of zero-copy restore. Publication supports an
exclusive in-memory owner and the optional whole-owner hosted lock; it does not
claim atomic writes into an arbitrary external state store.

`TimelineScope` declares a whole consistency domain. Authentication, complete
transitive dependency capture, migration, deterministic computation and finality
proofs belong to the consumer. The codec only validates metadata structure.
The SQLite demonstration does not persist a complete `LiveTimeline` recovery
image, and its cooperating endpoint does not establish universal exactly-once
delivery. [The wire boundary](timeline-wire-v1.md) and example documentation
state those limits explicitly.

## Verification disposition

The following local commands passed against the implemented production code:

- Stable workspace all-target/all-feature tests, doctests, strict Clippy and strict rustdoc.
- Rust 1.85.0 workspace all-target/all-feature tests; subsequent timeline boundary
  tests and capacity/throughput benchmarks also pass on that compiler.
- Bare-metal RISC-V checks for core default/Serde and timeline default/alloc.
- Layer checks and their negative controls, action pin checks, campaign-runner
  tests, Python script tests, formatting and whitespace checks.
- Coverage ratchet: 94.9863% lines against 94.0759%, measured before the two
  additional sparse-anchor/genesis tests. This is aggregate source coverage,
  not proof of exhaustive correctness or per-module threshold satisfaction.
- `cargo audit`, `cargo deny check advisories bans licenses sources` and the
  reachable-history secret scan (zero findings).
- Ten versioned package archives verified with a fresh temporary Cargo registry.
- Seven signed StateChronicle conformance cases against its pinned upstream,
  including real process kills and checkpoint evidence. No upstream changes.
- Each of five timeline model properties: 1,000 cases with seeds 401/402/403;
  4,096 short metadata traces and 1,200 short correction/fault cases.
- Each of five timeline state-machine fuzz targets: 10,000 seeded instrumented
  runs with mutation seed 403 and 100,000 with seed 404; wire separately has
  10,000 instrumented seed-401 runs with seven golden variants.
- Each of three inherited timed properties ran at least 300 seconds with rotating
  seeds and 10,000 requested cases per batch. Retry monotonicity completed 1,389
  batches, live/replay convergence 1,047, and crash/persistence chaos 75.

Remote [seven-job CI on `c88f704`](https://github.com/STEXS-Technologies/penelope/actions/runs/41293991270)
and [the later memory/boundary commit `1f1f50a`](https://github.com/STEXS-Technologies/penelope/actions/runs/41294649320)
pass. [Compressed local gate logs, timed property evidence and source hashes](verification-data/2026-11-01-timeline-gates/manifest.json)
retain the exact verification scope, including the later supplemental tests and
versioned packaging. The passing tagged-0.3.0 release workflow is also retained
there as foundation evidence. The completed repeated campaign is archived below.

The subsequent convenience API at `453039f` has
[separate completed gate logs and source hashes](verification-data/2026-11-01-timeline-devex/manifest.json):
workspace tests, final timeline Rust 1.85 all-target tests, strict docs/doctests,
1,000-case seed-405 properties, 100,000 instrumented correction runs with seed
405, fresh-registry packaging and coverage. Coverage is 95.0459% against the same
94.0759% ratchet. Portable default/alloc, strict timeline Clippy, layer negative
controls and the executable quickstart also pass. The original archive retains
its earlier revision scope. [Convenience performance and sampled-stack records](timeline-performance.md#convenience-api-follow-up)
are separate, and do not establish a universal overhead percentage.

The subsequent proposed-head validation audit at `83a51c6` aligns correction
validators with live-input validators. [Completed gate logs and source hashes](verification-data/2026-11-01-timeline-validation/manifest.json)
include the full stable workspace, core/timeline MSRV, strict Clippy/docs/doctests,
portable checks, packaging, 10,000-case seed-406 properties and two instrumented
100,000-run seed-406 fuzz campaigns. The regression fails when the old validator
argument is restored; that expected-failure negative control is labeled separately
from positive gates. Coverage is 95.0471% against the unchanged threshold.
[Fresh measurements](timeline-performance.md#proposed-head-validation-audit) retain
the final run and its earlier loaded preview. That archive predates completion
of the full repeated campaign.

The completed repeated command is the unchanged inherited release campaign:

```sh
PENELOPE_CHAOS_ITERATIONS=3 PENELOPE_CHAOS_PROPTEST_CASES=10000 \
PENELOPE_CHAOS_FUZZ_RUNS=100000 ./scripts/run_pure_chaos_drill.sh
```

[The completed campaign archive](verification-data/2026-11-01-timeline-chaos/manifest.json)
contains the compressed raw log, full audited summary, actual environment, auditor
and its positive/negative controls. All 22 instrumented fuzz targets completed
100,000 runs with each of seeds 1, 2 and 3: 66 batches and 6,600,000 executions.
All five timeline properties passed in each round; the runner exited 0.

The campaign started at `5c5330c`. Production evolved through `453039f` to
`83a51c6` during round one; rounds two and three used the final production code.
It is not a campaign against one frozen revision. The seed-406 supplements above
separately cover the final validator change. At campaign completion the production source hashes matched
the validation archive; the subsequent lookup optimization is qualified separately below. The verification-only release workflow on `83a51c6`
[passed](https://github.com/STEXS-Technologies/penelope/actions/runs/41299678117);
no release was published.

It retains all 22 fuzz targets and includes timeline properties in each round.
Real SQLite fuzz targets use private local databases; actual process-kill and
fencing tests remain in the workspace suites. No power-loss certification or
production deployment qualification follows from those checks.


## Captured-input optimization follow-up

The captured-input lookup now uses a checked logical-step slot hint before its
exact-record fallback. Full descriptor and per-read payload integrity checks
remain required; arbitrary array order, gaps, multiple versions and mixed scopes
are still supported. [The matched isolated-build speed audit](timeline-performance.md#captured-input-lookup-optimization)
retains baseline and optimized sources/binary hashes and all 391 profiles.

[Optimization qualification evidence](verification-data/2026-11-01-captured-input-optimization/manifest.json)
records stable timeline all-target/all-feature tests with 10,000 cases per
property (seed 407), two additional 10,000-case lookup properties, explicit
counter/descriptor/corruption regressions, Rust 1.85 timeline tests, portable
RISC-V default/alloc checks, strict Clippy/docs/doctests and layer negative
controls. The existing instrumented correction fuzzer, including scoped captured
input replay, completed 100,000 runs with seed 407. Workspace coverage tests pass
at 95.0537% lines against the unchanged 94.0759% ratchet. All 391 separately
instrumented optimized benchmark operations allocate zero Rust heap bytes.
The earlier full repeated campaign retains its earlier source scope; it is not
claimed as a repeated campaign against this subsequent optimization.

## Effect storage, unwind and slice audit

[The audit archive](verification-data/2026-11-01-timeline-audit/manifest.json)
qualifies the dense effect prefix and ordered bulk handoff described in the
[matched performance audit](timeline-performance.md#effect-buffer-audit).
An independent BTreeMap reference model checks interleaved append, correction,
finality, duplicate/validation refusal, bounded handoff and failures after partial
ACKs at capacities 0/1/8/32. Explicit tests cover bulk sorting with speculative
intents and partial ACK retry, plus consumer destructor unwind during compacting
and bulk handoff. Removed acknowledged intents remain uncharged on unwind.

The audit also reproduces and fixes a retirement cleanup leak: branch lineage
previously disappeared before all consumer payload destructors completed.
Retirement now leaves the leaf discoverable until cleanup finishes; callers can
retry after unwind. Four successive payload destructor panics preserve accounting,
parent history and branch-ID high-water invariants. The old implementation fails
the archived negative-control regression.

Captured input slices now work directly through scoped and low-level replay,
pinned sessions and live-step preparation. These entry points accept unsized
`InputStore` adapters while preserving the borrow that prevents eviction.
The old bounds fail the archived compile negative control; direct slice tests
and existing compile-fail pin doctests pass.

Completed local checks include all-feature timeline tests with 10,000 cases per
property (seed 410), strict all-target Clippy, docs/doctests, Rust 1.85 timeline
tests, portable RISC-V default/alloc, layer negative controls and fresh-registry
verification of all ten library archives. The instrumented effects fuzzer now
also runs the independent sequence model; 100,000 seed-410 runs completed.
Workspace coverage is 95.1590% against the unchanged 94.0759% threshold.
Temporary-directory quota failures were resolved with workspace-local `TMPDIR`;
successful reruns are archived separately from expected regression failures.
The earlier 66-batch campaign retains its earlier source scope.

## FixedHistory input lookup follow-up

[The lookup qualification archive](verification-data/2026-11-01-history-lookup/manifest.json)
covers checked step-relative slot hints, step-filtered exact fallback and direct
verification of canonical ancestry matches. Exact stored imports retain their
precedence over inherited aliases; full descriptor matching, scope/floor guards,
payload digest and declared-charge checks remain required. No public storage
layout fields, dependencies or allocation requirements change.

A linear immutable-version oracle checks interleaved canonical/imported versions,
arbitrary initial floors/branch identities, eviction and slot reuse. Counter-edge
cases reach `u64::MAX`; mutable-payload regressions detect both content and charge
changes on the hint and fallback paths. All 10,000 seed-411 cases pass. A separate
temporary checkout deliberately removes only the full-record hint filter: all
three new tests fail at runtime, with expected exit 101. That faulty source is
never applied to the working branch.

All-feature timeline tests, strict all-target Clippy, Rust 1.85 timeline tests,
portable RISC-V default/alloc checks, strict workspace docs, compile-fail pin
doctests, layer negative controls and fresh-registry verification of all ten
library archives pass. The existing instrumented correction fuzzer additionally
runs the lookup oracle; 100,000 seed-411 runs complete successfully. Workspace
coverage is 95.1906% against the unchanged 94.0759% threshold.

[The matched performance archive](timeline-performance.md#fixedhistory-lookup-follow-up)
retains identical benchmark source, isolated binaries, three runs per version,
all 520 profiles and zero-allocation records. Longer scalar replay windows improve
substantially; small-window results include slight unfavorable movement. The
original repeated 66-batch campaign retains its earlier source scope and is not
claimed against this follow-up.

## Inherited input-prefix resolution

[The ancestry qualification archive](verification-data/2026-11-01-ancestry-scan/manifest.json)
covers the guard that skips a full child-local canonical input scan within its
inherited prefix. Fork admission fixes that boundary; canonical child appends
start strictly after it, while imports are noncanonical. Exact stored imports,
corrected-input overrides, root lookup and post-fork child appends retain their
existing selection and integrity checks. Snapshot resolution is unchanged.

An independent canonical input-version vector for each retained branch checks
up to five child generations of state/input corrections, child-local appends,
unselected imported versions, parent preservation and branch materialization.
Both explicit mixed cases and 10,000 seed-412 property cases pass. The snapshots
are admitted fixtures; this oracle qualifies input selection, not arbitrary
consumer state completeness or authority. A detached negative control removes
only the fork-boundary condition: both new tests fail at runtime with expected
exit 101, catching omitted post-fork inputs. Faulty code never enters the branch.

Final-source checks include all-feature timeline tests with 10,000 cases per
property, strict all-target Clippy, Rust 1.85 timeline tests, RISC-V default/alloc,
strict workspace docs, compile-fail pin doctests, layer controls and fresh-registry
verification of all ten library archives. The instrumented correction fuzzer
retains its existing correction/flat-history models and adds the ancestry oracle;
100,000 seed-412 runs complete. Workspace coverage is 95.1909% against the
unchanged 94.0759% threshold. Archived `final-` logs qualify the final fallback
guard placement; initial candidate qualification retains only its earlier scope.

[Alternating performance measurements](timeline-performance.md#inherited-prefix-scan-follow-up)
retain all 520 profiles, exact source/binary hashes and zero-allocation records.
An unfavorable sparse-read median remains visible alongside overlapping run
ranges. This is not a claim that every path speeds up or a new repeated 66-batch
campaign; the original campaign retains its earlier production scope.

## Snapshot lookup and materialization audit

[The snapshot qualification archive](verification-data/2026-11-01-snapshot-lookup/manifest.json)
covers logical-step filtering before full exact-anchor comparison and direct
content/charge verification of canonical matches. Full descriptor checks and
exact-import precedence remain required. Unlike inherited input ownership,
canonical child snapshots can cover an inherited prefix; their lookup is retained.

An independent per-branch BTreeMap tracks canonical descriptors and fixture values
through three nested state corrections, local child snapshots on either side of
the correction boundary, explicit imports, parent preservation, corrected-away
anchors and detached materialized histories. Forged metadata and foreign scopes
are refused. All 10,000 seed-413 property cases and explicit prefix cases pass.
Admitted fixtures do not prove arbitrary consumer snapshot completeness or authority.

Mutation regressions detect canonical snapshot content and declared-charge changes
through both lookup and restore. A corrupt exact child import cannot silently
fall back to a healthy parent, while canonical lookup retains the healthy parent
fact. A detached negative control removes only the new canonical verification
call; the corruption regression fails at runtime with expected exit 101. Faulty
source never enters the working branch.

All-feature timeline tests with 10,000 cases per property, strict all-target Clippy,
Rust 1.85 timeline tests, portable RISC-V default/alloc, strict workspace docs,
compile-fail pin doctests, layer controls and fresh-registry verification of all
ten library archives pass. The existing instrumented correction/flat-history/
input-ancestry fuzz models additionally execute the snapshot oracle; 100,000
seed-413 runs complete. Workspace coverage is 95.2568% against the unchanged
94.0759% threshold. The earlier repeated 66-batch campaign retains its earlier
production scope and was not repeated against this change.

[The speed audit](timeline-performance.md#snapshot-lookup-and-materialization-follow-up)
retains all 560 profile rows for each run, same-binary controls, source/binary
hashes and zero-allocation records. Snapshot paths improve, while several input
controls move unfavorably. Same-binary variation does not establish that every
unfavorable movement is noise or prove noninferiority across all integrations.

## Detached materialization audit

[The materialization qualification archive](verification-data/2026-11-01-materialization/manifest.json)
covers direct slot construction in a fresh detached history. Source preflight,
source resolution, exact identities, clone digest validation, per-payload limits
and checked aggregate charge remain required. Actual clones can declare different
sizes from their source; final accounting uses the validated clone sizes.

Three new regression tests also pass against baseline production `2e83d59`.
For each of five clone positions, they reject changed content, oversized payloads,
aggregate quota excess and u64 overflow, while accepting and charging valid larger
or smaller clones. A corrupt final source snapshot is refused before any clone.
Injected clone panics at every position preserve source readability and allow
retry. Shared test instrumentation and intentional corruption do not relax the
consumer contract forbidding mutable aliases to admitted payloads.

A detached negative control replaces only the materialized input clone integrity
check with its declared byte length. The new clone-validation regression fails
at runtime with expected exit 101; faulty code never enters the working branch.

All-feature timeline tests with 10,000 cases per property and seed 414 pass,
including the independent ancestry and snapshot oracles that materialize selected
branches. Strict all-target Clippy, Rust 1.85 tests, portable RISC-V default/alloc,
strict workspace docs, pin doctests, layer controls and fresh-registry package
verification of all ten libraries pass. The existing instrumented correction,
flat-history, input-ancestry and snapshot fuzz models complete 100,000 seed-414
runs. Workspace line coverage is 95.2713%, above the unchanged 94.0759% ratchet.
The earlier repeated 66-batch campaign retains its earlier production scope.

[The corresponding performance evidence](timeline-performance.md#detached-materialization-follow-up)
retains all 560 profiles, including unfavorable controls. All eight measured
materialization profiles improve; all 560 instrumented operations remain free of
Rust heap allocations. This does not prove arbitrary consumer snapshot validity,
determinism or integration-wide speed.

## Snapshot state-width qualification

[The state-width qualification archive](verification-data/2026-11-02-snapshot-width/manifest.json)
covers the expanded 640-profile benchmark with scalar, 1 KiB and 4 KiB states.
Library production source remains `735f809`; this follow-up adds benchmark
coverage and evidence for the existing optimization.

The same benchmark source runs against baseline `2e83d59` and optimized production.
For each state width, branch layout and snapshot count, fixture assertions verify
canonical snapshot selection and complete restored state. Outside timing, root
and inherited materializations additionally verify their selected canonical
descriptor, complete state and all 512 aliased input payloads. This is a synthetic
integer model; it does not prove arbitrary consumer completeness or determinism.

Stable all-target/all-feature Clippy passes with warnings denied. Rust 1.85 both
compiles the all-feature benchmarks and executes all 640 profiles with 100 samples
each; those smoke measurements are excluded from the stable latency aggregates.
All-feature timeline tests pass with 1,000 cases per property and seed 415. All
640 instrumented operations allocate zero Rust heap bytes, and all 39 capacity
rows match the preceding archive. Prior production fuzz, coverage and portability
evidence retains its original source scope; no new campaign is claimed here.

[The performance report](timeline-performance.md#snapshot-state-width-qualification)
records gains across all 24 materialization profiles, with smaller relative gains
for larger snapshots. It retains every raw control, including unfavorable
movements. These results do not establish a universal library overhead percentage
or a whole-game speedup.
