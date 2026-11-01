# 0.4.0 branch qualification

The implementation starts from released `v0.3.0`,
`788c282e359477469cd805c7ea82f2da22f85a87`. This audit covers the rewindable
timeline branch, not authorization to merge, tag or publish a release.
The repeated full chaos campaign is still running; qualification is incomplete
until its recorded command exits successfully. Other evidence below retains its
own source revision and profile limits.

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
there as foundation evidence. The repeated-chaos result remains outstanding;
its logs are under ignored `target/timeline-foundation/qualification-*` paths.

The outstanding repeated command is the unchanged inherited release campaign:

```sh
PENELOPE_CHAOS_ITERATIONS=3 PENELOPE_CHAOS_PROPTEST_CASES=10000 \
PENELOPE_CHAOS_FUZZ_RUNS=100000 ./scripts/run_pure_chaos_drill.sh
```

It retains all 22 fuzz targets and includes timeline properties in each round.
Real SQLite fuzz targets use private local databases; actual process-kill and
fencing tests remain in the workspace suites. No power-loss certification or
production deployment qualification follows from those checks.
