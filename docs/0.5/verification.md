# Verification and release qualification

**The 0.5 candidate is not release-qualified or published.** This page separates
current passing gates from open work and revision-scoped development evidence.
The [plan](plan.md) defines acceptance; [release commands](../RELEASE.md) define
how to run the gates. Tagging and publication require separate authorization.

## Current candidate

All ten library crates use candidate version 0.5.0. Their code, manifests, locks,
CI and scripts are identical at `9a5a1db` and `be774e6`; intervening changes only
add documentation and evidence. The
[candidate checkpoint](evidence/packaging/candidate-checkpoint.json) contains
commands, source/archive identities, raw logs and the complete LLVM JSON report.

| Gate | Verified result and scope |
| --- | --- |
| CI | All seven jobs pass at [`be774e6`](https://github.com/STEXS-Technologies/penelope/actions/runs/43794270936): stable, MSRV, bounded fuzz, signed-source conformance, portable core, supply-chain and secrets |
| Library tests | All-target/all-feature tests pass on Rust 1.100 and MSRV 1.89; stable doctests, strict workspace/compatibility Clippy and warning-denying API docs pass |
| Published-content isolation | All ten actual 0.5 archives pass Cargo verification and literal-include checks; extracted-workspace all-target/all-feature tests pass on both compilers, with every Penelope dependency resolved from those archives |
| Locks | Six active locks retain every external package identity and checksum; extracted-workspace normalization removes only fuzz-only `arbitrary`, `jobserver` and `libfuzzer-sys`, then both compiler runs use the same lock with `--locked` |
| Frozen compatibility | 33,278 task/timeline observations and 3,136 saga observations match original 0.4; both legacy and prepared saga facade paths pass on both compilers; frozen corpora and baseline locks are unchanged |
| Real database upgrade | An actual 0.4 writer populates 15 legacy tables; upgrade and two reopens preserve exact rows, replay and integrity on both compilers |
| Conformance | All seven selected signed-source tests pass against pinned StateChronicle `ae3d94bc`; no unpinned upstream is substituted |
| Portable profiles | Core bare/serde and timeline bare/alloc compile for `riscv32imafc-unknown-none-elf` on both compilers; CI separately runs allocation checks |
| Consumer progression | First-machine tutorial, flat, nested, decision, timeline and durable machine/task/saga binaries run on both compilers |
| Repository checks | All six checkout-only architecture tests run in the compatibility harness; layer boundaries, action pins and formatting pass |
| Coverage | **14,874 / 15,807 lines = 94.0976%**, above the unchanged **94.0759%** ratchet; all 72 reported source files belong to the candidate checkout |

The archive retains unsuccessful attempts too: an unpinned conformance checkout
was correctly refused, and two accidental unbounded fuzz launches caused by an
omitted library-test exclusion were deliberately stopped. The corrected pinned
conformance and documented library commands passed. Neither attempt changed the
separate bounded chaos rehearsal. Benchmark debug smokes are not release timing
measurements, and line coverage is not branch completeness or a safety proof.

## Open release gates

- Complete the full campaign on the immutable `be774e6` checkout: three rounds,
  10,000 property cases and 100,000 requested fuzz executions per target, with
  per-round seeds 1/2/3 and pre/post source hashes. CI's shorter bounded fuzz run
  does not replace it. The older development rehearsal remains separate evidence.
- Resolve findings from the completed versioned same-workload performance
  comparison against original 0.4 (`42ec187`): 31 alternating pairs across all
  eight selected legacy benchmarks, with 1,000 samples per timeline profile.
  Retain every finding and resolve review blockers under
  [plan §13.2](plan.md#132-baseline-performance-methodology).
- Finish the requirement-by-requirement maintenance/release audit, dated release
  notes and final source/artifact reconciliation. Passing earlier checkpoints
  does not qualify later source automatically.

No regression acceptance is recorded. Diagnosis, intervals spanning zero and a
smaller number of flags do not by themselves establish equivalence or waive a
sustained regression. Coverage passes narrowly; its margin has not improved over
the preceding failure-path checkpoint.

## Performance evidence

These measurements describe their recorded development revisions, not a completed
performance qualification of the versioned candidate. Same-work comparisons keep
original fixtures and independent locked builds. CPU affinity, cache policy,
shared-host interference and resource scope are recorded; no isolated-host or
worst-case latency guarantee is implied.

| Workload or finding | Retained evidence and disposition |
| --- | --- |
| Versioned 0.5 versus 0.4 | [Complete 31-pair comparison](evidence/legacy-balanced/versioned-candidate.json): all 496 measured processes and 16 binary identities retained. DTO encoding loop means are 312.95 → 357.19 ns (+14.14%, 27/31 pairs over 5%); timeline retains 31 p50 and 9 p95 flags, including five handwritten Ring control flags. Saga/task loop-mean profiles do not cross the review threshold; that does not establish equivalence |
| Inherited reads in the versioned candidate | One-word/one-step inherited collection p50 is 211 → 291 ns (+37.91%); p95 is 231 → 301 ns (+30.30%), each crossing its threshold in 29/31 pairs. Additional inherited horizons/correction paths are flagged. Timeline production source and the shared core helper are unchanged since `94a7396`; this observation does not identify the cause or waive the new findings |
| Legacy saga/task/timeline | [Initial comparison](evidence/legacy-performance/manifest.json), [task/timeline extension](evidence/legacy-runtime/manifest.json), [31-pair follow-up](evidence/legacy-balanced/manifest.json): raw observations and all flags remain retained; process loop means and timeline p50/p95/p99 are distinct measurements |
| DTO encoding | The 31-pair development comparison measured 306.56 → 354.14 ns median **process loop means** (+15.52%, about 48 ns); this is not an individual-call percentile. [Layout controls](evidence/legacy-balanced/encoding-layout.json) reproduce sensitivity with identical text bytes; [inlining diagnostic](evidence/legacy-balanced/encoding-inline.json) rejects an unstable hint. No production hint or linker workaround is retained |
| Legacy timeline | [Later 31-pair comparison](evidence/legacy-balanced/timeline-current.json) retains history-capture p50 251 → 270 ns and history-append p95 291 → 321 ns flags. Its flagged `ring_replay` profile is a handwritten control, not a Penelope replay API call; it is neither subtracted nor used to waive library findings |
| Capture placement | [Focused placement experiment](evidence/legacy-balanced/timeline-capture-layout.json) reverses which revision is faster. Its filtered fixture is diagnostic, not a replacement for the frozen full benchmark; normalized assembly equality covers one helper, not the whole program |
| Legacy SQLite | [Balanced comparison and patched-dependency controls](evidence/sqlite-balanced/manifest.json) retain substantial replay/exact-retry costs. Patched original dependencies reproduce much of that cost; attribution does not accept it. No security downgrade or weaker validation is used |
| Timeline lookup | [Lazy validated hints](evidence/timeline-lookup/manifest.json) improve post-eviction paths while preserving original successful-hint behavior, ancestry/error precedence, digest checks and scan fallback. [All descriptive flags](evidence/timeline-lookup/lazy-triage.json.gz) remain available; the slower eager hint was rejected |
| Durable machine | [Store profiles](evidence/machine-store-performance/manifest.json) cover single/contending writers and recovery. [Read-only preflight](evidence/machine-preflight-performance/manifest.json) resolves known stale/conflicting/duplicate requests before writer admission, then fully revalidates new work under the write transaction. The 32-writer race improves locally; writers that all observe the old head can still queue, and tail latency is not bounded |
| Timeline over machine | [Adapter/native measurements](evidence/machine-timeline-performance/manifest.json) separate ownership, context and provenance cost; the comparator is the existing engine at that revision, not an original 0.4 binary |
| Native end-to-end | [Protected pipeline](evidence/native-pipeline-performance/manifest.json) measures commit, real worker, lost-ACK recovery, verified result and subsequent saga admission. It is sequential at `max_live=1`, not throughput, isolated worker CPU or all-payload saturation qualification |
| Journal pressure | [Bounded queue profiles](evidence/machine-backpressure-performance/manifest.json) exercise held-writer refusal, exact results and integrity. Queue refusal latency is a local observation, not a runtime saturation SLA |

### Flat machine measurement fixture

The [flat-machine measurements](evidence/machine-performance/manifest.json),
[hierarchy/depth measurements](evidence/machine-performance/hierarchy.json) and
[owned-state cost profiles](evidence/machine-performance/state-cost.json) retain
raw samples, allocations, source hashes and MSRV observations. They separate
bounded dispatch/staging/publication from user-owned 8 B–64 KiB state copying.
Allocation-free consumer types show zero internal heap allocation in the measured
core paths; consumer allocation or arbitrary user computation is not bounded by
that result. These are new capabilities, not ratios against a nonexistent 0.4
machine API.

## Guarantee and maintenance evidence

The following component checkpoints preserve independent models, mutations,
crash boundaries, errors and source identity. Their manifests define the exact
revision and scope; current candidate tests recheck the implemented paths, while
older measurements retain their original meaning.

| Boundary | Evidence |
| --- | --- |
| Original baseline and deviations | [Initial manifest](evidence/manifest.json), [completed original-source chaos](evidence/baseline-complete-chaos/), [toolchain checkpoint](evidence/rust-toolchain/manifest.json). Baseline failures for RUSTSEC-2026-0459 and RUSTSEC-2026-0486 remain recorded; candidate dependencies are patched, with approved MSRV 1.89 |
| Portable machine and publication | [Kernel/hierarchy](evidence/machine-checkpoint/), [shared admission extraction](evidence/staging-extraction/), [prepared saga/task views](evidence/prepared-adapters/): consuming publication, bounds, poisoning, panic/error precedence and native authority boundaries |
| Timeline reuse | [Machine/timeline composition](evidence/timeline-machines/): existing correction/finality machinery, rather than a second rewind engine |
| Wire and journal | [Independent metadata goldens](evidence/machine-wire/), [SQLite journal/snapshots](evidence/machine-storage/manifest.json): machine v1 stays separate from legacy process/task/timeline formats |
| Durable handoff | [Outbox](evidence/machine-outbox/manifest.json), [native task admission](evidence/machine-task/manifest.json), [native saga admission](evidence/machine-saga/manifest.json): full source binding, deduplication, leases, authorization and native lookup; no external-effect guessing |
| Complete progression | [Consumer recovery](evidence/coherent-consumer/manifest.json), [verified worker result](evidence/machine-workflow/manifest.json): durable machine → native task → verified result → subsequent machine transition → saga admission |
| Compatibility and migration | [Frozen replay/coverage checkpoint](evidence/compatibility-coverage/manifest.json), [actual writer upgrade and backup restore](evidence/sqlite-upgrade/manifest.json); upgrade/rollback obligations remain in the [compatibility matrix](compatibility-matrix.md) |
| SQLite failure isolation | [Preflight/history failure paths](evidence/qualification/machine-failures.json): intervening writer after read preflight, exact duplicate/conflict/stale outcomes, unchanged rows and bounded-history retry. Negative controls remove revalidation/history limits and fail; process-kill coverage includes eight commit boundaries |
| Native capacity | [Worker slot reuse](evidence/qualification/native-slot-reuse.json): real supervisor admission at `max_live=1`, exact retries, refused second task, generation fencing and reuse; correctness at that capacity does not qualify general saturation performance |
| Archive boundary | [Original archive verification](evidence/packaging/manifest.json), [shared fixture materialization](evidence/package-fixtures/manifest.json), [independent development archive tests](evidence/packaging/independent-tests.json). Two checkout-dependent executor tests failed initially; all six repository tests moved byte-identically to CI, without dropping a check |
| Onboarding | [Canonical tutorial qualification](evidence/qualification/first-machine.json): generated consumer source comes from the guide's sole Rust fence, including CRLF and clean application checks; no second hand-maintained tutorial implementation |
| Maintenance | [Inventory and owner audit](evidence/maintenance/manifest.json): 14,356 → 18,867 potential production source lines at `9a5a1db`, +101 across existing paths, two unchanged exact whole-function clone groups; 72 → 69 declared dependency edges and 171 → 59 tracked Markdown files. New capabilities grew production source; counts do not prove lower complexity or absence of semantic duplication |
| Rejected simplifications | [Task-reader experiment](evidence/maintenance/task-read-evaluation.json): fewer lines did not justify slower reads. [Inherited-prefix probe](evidence/maintenance/inherited-lookup-evaluation.json): independent models and negative controls pass, but faster inherited reads accompany a repeated root-collection regression; the probe remains outside candidate code |
| Earlier broad rehearsal | [Workspace qualification](evidence/qualification/manifest.json): earlier coverage, conformance, migration, consumer and supply-chain results retain their recorded source scope |

[Architecture owners and retained boundaries](architecture.md#adr-050-06-extraction-boundaries)
explain what is shared and why specialized task generations, saga compensation,
timeline finality and native transactions remain separate. Sharing inspection or
candidate vocabulary does not create universal execution authority.

## Evidence policy

Raw archives, manifests, frozen baselines and unsuccessful attempts remain
unchanged by this documentation consolidation. Every evidence target from the
previous page remains linked here. The full chronological account is preserved
at [commit 97c5c3c](https://github.com/STEXS-Technologies/penelope/blob/97c5c3cb6b34c4dcd9589726e8af0a79ab2c7283/docs/0.5/verification.md).

Actual worker/process-kill evidence uses Linux x86-64. Bare-metal checks prove
compilation, not execution on physical RISC-V hardware. SQLite crash tests do not
establish arbitrary power-loss or storage-device guarantees. Unknown external
outcomes still require reconciliation; durable acceptance is not exactly-once
external execution. The [current status](README.md) remains open until the full
release definition of done is evidenced.
