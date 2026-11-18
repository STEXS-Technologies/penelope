# Verification and release qualification

**The 0.5 candidate is not release-qualified or published.** This page separates
current passing gates from open work and revision-scoped development evidence.
The [plan](plan.md) defines acceptance; [release commands](../RELEASE.md) define
how to run the gates. Tagging and publication require separate authorization.

## Current candidate

The `238380b` candidate includes a timeline optimization that borrows lineage and
optional override records instead of copying them on every lookup. Its
[local source/artifact reconciliation](evidence/qualification/lineage-borrow-qualification.json)
records the exact patch over `7ab93d2`, matching all 449 tracked crate/workspace
inputs between this checkout and the isolated qualification tree. Both compiler
workspace runs report 700 passes. Ten final archives verify, extracted-workspace
checks pass on both compilers, and final packaged executor tests include the
strengthened admission-race and receipt-read failure assertions. Strict Clippy,
warning-denying API docs and the unchanged coverage ratchet pass. The
[clean `238380b` checkpoint](evidence/qualification/lineage-clean-checkpoint.json)
now retains all seven green CI jobs, ten freshly verified clean archives and
member-level reconciliation to the tested packages. Every member matches except
recorded VCS identity and any reconciled internal lock checksums; no new
700-check extracted run is claimed. All 449 source inputs match the local
qualification, including all 72 coverage-reported production hashes. All 58
indexed named checks are observed in both current compiler CI jobs. The clean
`7ab93d2` results below retain their earlier scope. Rejected SQLite and serializer
experiments are not integrated.

All ten library crates use candidate version 0.5.0. The
[preceding clean source/artifact reconciliation](evidence/qualification/pre-pr-reconciliation.json)
verifies ten clean archives at `7ab93d2`, with 700 reported passing checks on each
of Rust 1.100 and MSRV 1.89, explicit packaged-example runs, fresh coverage and
seven green CI jobs. All 72 coverage-reported production files match the preceding
checkpoint. The only changed crate input is the SQLite test helper's fuzz-only
timeout adjustment; the fresh archives contain that adjustment. The record retains
the clean source, package/lock identities, raw logs, full LLVM JSON and an initial
recorder interruption caused by interleaved child-process summary output. Cargo
passed; the corrected recorder reused that successful stable run and completed
the remaining compiler/example checks. **Full release qualification remains open.**

The earlier [graph-start checkpoint](evidence/qualification/graph-start-package-checkpoint.json)
retains its `8dfe89d` source and archive identities. The current reconciliation
checks all 514 plan-review blocks, 656 JSON pointers and 63 other evidence targets,
and observes all 58 indexed named checks in both current compiler CI jobs.
Historical hash differences retain their source scopes and explicit dispositions;
reference validity and matching tests do not prove semantic completeness.

The [graph-start ownership fix](evidence/qualification/graph-start-ownership.json)
removes an unnecessary definition borrow while retaining the original function
body. Its facade consumer fails with the old signature and passes on both
compilers after the change. The current checkpoint reconciles its packages,
coverage and CI; final campaign qualification remains open.

Earlier [nested](evidence/qualification/nested-ergonomics.json) and
[flat enum](evidence/qualification/door-package-checkpoint.json) follow-ups retain
their original checks and failed attempts. The
[c8fda4c checkpoint](evidence/qualification/current-package-checkpoint.json) and
[history-partition checkpoint](evidence/qualification/history-partition-checkpoint.json)
retain their earlier source scopes.

| Gate | Verified result and scope |
| --- | --- |
| CI follow-up | Later example changes exposed two workspace-only Clippy errors in CI at `f36b77d`. The [lint correction](evidence/qualification/workspace-example-lint.json) passes the full stable workspace check and preserves both compiler runs and all six event traces; the current checkpoint includes subsequent green CI and clean archive checks |
| CI | All seven jobs pass at [`238380b`](https://github.com/STEXS-Technologies/penelope/actions/runs/44214450867): stable, MSRV, bounded fuzz, signed-source conformance, portable core, supply-chain and secrets |
| Library tests | All-target/all-feature tests pass on Rust 1.100 and MSRV 1.89; stable doctests, strict workspace/compatibility Clippy and warning-denying API docs pass |
| Timed legacy properties | [Three registered properties](evidence/qualification/qpsg-property-stress.json) pass at `cefa55f`, each for at least 300 seconds with rotating seeds and 10,000 requested cases per batch. Exact registration, per-test logs and unchanged source hashes are retained; this does not replace full machine/timeline chaos |
| Generated machine models | [Supplementary test/fuzz qualification](evidence/qualification/generated-machine-models.json): varied bounded transition tables and parent forests, exact provenance/intent/path oracle, one-level flat equivalence, stable/MSRV tests and two caught production mutations. The augmented machine target completes three independent 100,000-run fuzz rounds. That supplementary run changes only test/fuzz inputs and retains its `cefa55f` production scope. The [clean core archive](evidence/qualification/generated-machine-package.json) at `9a8134f` includes both new files and passes the extracted test on both compilers. [Fresh coverage](evidence/qualification/generated-machine-coverage.json) runs the added test and retains the same 94.0875% line result; the current checkpoint includes both additions in the full extracted workspace |
| Machine history partitions | [Exhaustive small-history check](evidence/qualification/machine-history-partitions.json): all 256 partitions under both count and byte limits, reopen between pages, a pinned upper revision after later writes and exact restored payload/state/intent/receipt data. All 14 machine-journal tests pass on stable/MSRV; removing continuity validation is caught. Test-only addition; the current checkpoint includes the test in both extracted-workspace runs and fresh coverage |
| Replay-mutation fuzz follow-up | [Initial checks](evidence/qualification/replay-mutation-initial.json) pass for the augmented SQLite target: ten deterministic seeds, 1,000-run smoke, strict Clippy and formatting. Reverse exact retries, future-head refusal and checksummed scope/model/event-digest substitutions are exercised. The [supplementary campaign](evidence/qualification/replay-mutation-complete.json) completes all three 100,000-run rounds with unchanged source/binary and retained terminal corpora. Its `f36b77d` base predates isolated event replay; it does not qualify that newer method |
| Event replay | [Recovery follow-up](evidence/qualification/event-replay.json): original events are reevaluated through normal preparation, with complete regenerated record/payload comparison and discarded intents. Stable/MSRV ports tests and SQLite history tests pass; all 512 small-history pagination configurations now also check replayed state. Two compiled negative controls must fail. The current checkpoint includes packages, coverage and CI. The [supplementary SQLite campaign](evidence/qualification/event-replay-fuzz-complete.json) completes three 100,000-run rounds at `9084239`, with actual model-reevaluation replay, retained corpora and unchanged source/binary. Its production scope remains `9084239`; the later graph-start lifetime refinement is qualified separately. Full-source campaign qualification remains open |
| Repeated journal kills | [Growing-journal crash/model check](evidence/qualification/repeated-machine-kill.json): three seeds, 16 successive SIGKILLs per database, all eight commit boundaries twice per seed. Both compilers pass all nine machine unit tests; every retained prefix, payload/intent, row count and exact retry is checked after each death and reopen. Test-only addition; the current archive checkpoint includes it on both compilers, separately from clean-reopen fuzzing |
| Published-content isolation | All ten actual 0.5 archives pass Cargo verification and literal-include checks; extracted-workspace all-target/all-feature tests report 97 harness summaries and 700 passes on each compiler, with the added replay and repeated-kill tests observed and every Penelope dependency resolved from those archives |
| Locks | Extracted-workspace normalization removes only fuzz-only `arbitrary`, `jobserver` and `libfuzzer-sys`; every retained external identity/checksum is unchanged and both compiler runs use the same lock with `--locked`. Earlier six-lock inventories retain their recorded scope |
| Frozen compatibility | 33,278 task/timeline observations and 3,136 saga observations match original 0.4; both legacy and prepared saga facade paths pass on both compilers; frozen corpora and baseline locks are unchanged |
| Static API review | [Fourteen package/feature profiles](evidence/maintenance/public-api-audit.json) retain the checked original export bindings and pass the local-item signature checks. Independent export traversal and the unchanged consumer catch a facade reexport removal missed by the signature checker; a public-field removal is also caught |
| Real database upgrade | An actual 0.4 writer populates 15 legacy tables; upgrade and two reopens preserve exact rows, replay and integrity on both compilers |
| Conformance | All seven selected signed-source tests pass against pinned StateChronicle `ae3d94bc`; no unpinned upstream is substituted |
| Portable profiles | Core bare/serde and timeline bare/alloc compile for `riscv32imafc-unknown-none-elf` on both compilers; CI separately runs allocation checks |
| Flat enum example | [Local ergonomics follow-up](evidence/qualification/door-ergonomics.json): one non-Clone state enum with variant-local data, one event enum and one `Machine` implementation. The same source runs as a facade-only consumer on Rust 1.100/MSRV 1.89. An eight-event oracle checks decisions, guard codes, state, revision and provenance; four compiled semantic mutations are caught. At `c0e9997` all 72 production sources and both locks are unchanged. This example retains explicit fixture metadata and stages no effects. Its [all-ten archive and CI follow-up](evidence/qualification/door-package-checkpoint.json) passes on both compilers |
| Nested example | [Eight ergonomic criteria](evidence/qualification/nested-ergonomics.json): one checker asserts all six complete traces; root fallback, terminal leaf refusal, guards, parent transition and self-reentry. Stable/MSRV pass; wrong guard/effect and removed leaf refusal are caught. A non-Clone nested enum with variant-local data now participates in the zero-allocation probe. Both compiler CI jobs run the checker; the current checkpoint also verifies exact packaged example/fixture bytes |
| Shared decision example | [Graph view addition](evidence/qualification/graph-example.json) extends the existing machine/linear-saga/task example, with source-consumer runs and strict Clippy on stable/MSRV. That example follow-up records unchanged production at its source revision; the current all-ten archive checkpoint includes the example |
| Consumer progression | First-machine tutorial, flat, nested, decision, timeline and durable machine/task/saga binaries run on both compilers |
| Repository checks | All six checkout-only architecture tests run in the compatibility harness; layer boundaries, action pins and formatting pass |
| Coverage | Local timeline/test patch: **14,925 / 15,864 lines = 94.0809%**, above the unchanged **94.0759%** ratchet; all 72 reported source hashes match this checkout. Earlier `7ab93d2`: **14,925 / 15,860 = 94.1047%** |

Earlier unsuccessful attempts remain in the preceding checkpoint. Current
commands use the pinned conformance source and exclude the separate fuzz package
from library tests. Debug benchmark smokes are not release timing measurements,
and line coverage is not branch completeness or a safety proof. The earlier
history-partition checkpoint retains a rejected attempt that reused an older
compiled target and omitted its new history test. Its successful command exits
were not accepted as qualification. Both that follow-up and the current checkpoint
use a fresh target, explicit added-test observations and retained test-name
comparison; subprocess-split output is documented in the recorders.

## Open release gates

- Qualify the corrected immutable library sources: three rounds,
  10,000 property cases and 100,000 requested fuzz executions per target, with
  per-round seeds 1/2/3 and pre/post source hashes. CI's shorter bounded fuzz run
  does not replace this. The already-running `9084239`/`cefa55f`/`be774e6`
  campaigns retain their original source/harness scopes. The
  [development rehearsal](evidence/qualification/development-chaos-complete.json)
  passed all 84 fuzz batches across three rounds. Its checkout advanced during
  execution, so it supplies no immutable-revision qualification. None of these
  campaigns is relabeled as the final revision.
  The [first final-source attempt](evidence/qualification/final-chaos-toolchain-interruption.json)
  stopped on a compiler-version tooling error after 21 fuzz batches, with unchanged
  source hashes. A fresh attempt uses installed `nightly-2026-08-05` through a
  retained invocation wrapper; its 1,000-run worker-protocol preflight passes.
  The subsequent pinned run at `8dfe89d` stopped after 53 of 84 batches when an
  outbox fuzz input exceeded the shared test helper's 10-second wall-clock
  deadline; libFuzzer recorded that unit at 23 seconds. The archived crash and
  slow-unit inputs both replay in under 100 ms, so the exact failure does not
  reproduce outside the long campaign. The [failure record and raw archive](evidence/qualification/final-chaos-deadline-failure.json)
  preserve the failed scope and inputs. The harness now allows 60 seconds only
  in fuzz builds (regular tests retain 10 seconds), and the bounded-fuzz runner
  sets libFuzzer's per-input limit to 90 seconds. The corrected target builds,
  both archived inputs replay, and a seeded 1,000-run smoke completes 2,165
  executions. These checks do not close full qualification; the complete
  three-round campaign must run again on the corrected immutable source.
  Before the fix, branch tip `a068f13` had matching SHA-256 values for all 480
  selected build/runtime inputs in the [branch input comparison](evidence/qualification/current-branch-input-equivalence.json);
  this establishes input equality, not commit identity. The corrected source's
  immutable run remains open.
- Resolve findings from the completed versioned same-workload performance
  comparison against original 0.4 (`42ec187`): 31 alternating pairs across all
  eight selected legacy benchmarks, with 1,000 samples per timeline profile.
  Retain every finding and resolve review blockers under
  [plan §13.2](plan.md#132-baseline-performance-methodology). The
  [post-QPSG SQLite runner](evidence/sqlite-balanced/post-qpsg-candidate.py)
  now records 31 alternating durable-store pairs against the frozen 0.4
  baseline. The paired [result summarizer](evidence/sqlite-balanced/summarize-post-qpsg.py)
  was exercised against the archived 31-pair report format; it validates every
  report and retains medians, paired flags and exploratory bootstrap intervals.
  The [current-source comparison](evidence/sqlite-balanced/post-qpsg-current.json)
  is now complete at `7ab93d2`: all 62 measured reports, warmups, both binaries
  and source inventories are retained. Warm replay and exact retry do not cross
  their thresholds in this run. Two flags remain: 16-record reopen/replay p50
  287,095 → 317,694 ns (+10.66%, 27/31 pairs above 5%) and 16-record append p95
  695,414 → 818,322 ns (+17.67%, 14/31 pairs above 10%). The latter is the maximum
  of only 14 append observations per process. No flag is waived by that limited
  sampling or its exploratory interval. The comparison ran without a new chaos
  campaign; it does not close or waive the separate campaign obligation.
  The [fresh eight-workload legacy comparison](evidence/legacy-balanced/current-source.json)
  also retains 496 measured processes at `7ab93d2`, 16 binaries and unchanged
  fixtures. Its 40 flags include six handwritten controls. The
  [`7ab93d2` disposition inventory](evidence/qualification/current-performance-review.json)
  lists all **36 library review flags** across both comparisons; none is accepted.
  The later [31-round timeline borrowing comparison](evidence/legacy-balanced/lineage-borrow.json)
  retains all 640 profiles at 1,000 observations each, original/candidate/probe
  binaries and code-generation diagnostics. Selected inherited-read p50 improves
  18.42–30.69% versus `7ab93d2` and 3.29–5.41% versus original 0.4; no inherited-read
  threshold flag remains in that run. Seven other library p50 flags remain.
  The [per-profile follow-up](evidence/qualification/lineage-performance-followup.json)
  maps all historical findings to the later evidence without granting acceptance.
  It is not a new eight-workload final-source qualification or proof that an
  unflagged historical profile is equivalent.
- The requirement-reference and source/package reconciliation is complete for
  `238380b`, with open acceptance conditions preserved. Its ten clean archives
  reconcile to the tested local patch, and all seven CI jobs pass. Final performance
  disposition, full campaign qualification and dated release notes remain open.
  Later code changes or the eventual release revision require their own source
  and artifact reconciliation. The
  [release preflight](evidence/qualification/release-preflight.json) confirms
  ten `0.5.0` packages and dependency order; the current reconciliation independently
  rechecks both. The dated changelog entry remains deferred until qualification
  is complete. This stabilization pass deliberately excludes a new full chaos
  campaign and performs no tagging or publication.

An [owning-serializer experiment](evidence/legacy-balanced/owned-serializer-rejected.json)
was rejected after 31 rotating original/candidate/probe comparisons. Its DTO
loop-mean medians were 309.91 / 355.15 / 361.51 ns. Both compiler test suites,
golden wire checks and allocation checks passed, but the probe did not improve
the frozen workload. The code was not integrated and the DTO finding remains open.

A [current-schema open experiment](evidence/sqlite-balanced/current-schema-open-rejected.json)
was also rejected after 31 rotating original/candidate/probe comparisons. Avoiding
the migration writer lock for version 9 improves 16-record reopen/replay p50 by
only 1.34% versus the candidate and remains 7.06% slower than original 0.4.
All 61 SQLite unit tests passed on both compilers, including held-writer and
future-version controls. Correctness passed, but the performance finding remains
open and the schema change was not integrated.

No regression acceptance is recorded. Diagnosis, intervals spanning zero and a
smaller number of flags do not by themselves establish equivalence or waive a
sustained regression. Local patch coverage passes at 94.0809%; both preceding
failed reports (94.0683% and 94.0746%) are retained. The executor race now forces
both initial empty reads and exactly one native conflict. The lost-ACK check
also refuses source acceptance during an unavailable native receipt read and
then retries the same receipt successfully. No coverage threshold was lowered.
The preceding `7ab93d2` coverage passes at 94.1047%, compared with the
event-replay checkpoint's 94.1173%. The two fewer covered lines are confined to
the SQLite payload adapter (78/91 versus 80/91); all other production file line
totals are unchanged. The fresh result is retained. Aggregate coverage does not
prove every new branch is exercised. The 15-invariant audit retains its recorded 27 owner
source hashes and 33 boundary-test references. The replay addition has separate
current evidence; the current clean-archive runs include those added replay checks.

The [phase work audit](evidence/maintenance/phase-audit.json) maps all 50 work
items and nine gates in plan §11 to 67 source/evidence files. It distinguishes
scoped implementation support from open performance/release work and unproven
historical sequencing. The whole-plan index below adds the separate clause and
named-test reviews; none is a completion certificate. The
[required test-layer audit](evidence/maintenance/test-layer-audit.json) maps all
28 clauses in §13.1 to 29 observed stable/MSRV tests and 20 archived external
gates. It retains the live campaign obligation and distinguishes seeded process
kills from clean-reopen fuzzing. Its earlier targeted replay-mutation gap now has
separate completed supplementary follow-up evidence above.

The [integration contract audit](evidence/maintenance/integration-contract-audit.json)
maps all 20 numbered saga/task/timeline work items in §6.1–6.3 to 29 source files
and 13 named tests observed on both compilers at `7d24718`. It also retains the
real-worker progression and wrong-result refusal from stable CI. Native authority
boundaries and conditional extraction decisions remain explicit; the orchestration
performance item and final release qualification stay open.

The [whole-plan review index](evidence/maintenance/plan-review-index.json) covers
all 514 prose, table and illustrative-API blocks. It records actual owners,
file-name/API refinements, deployment duties and scoped evidence, including 58
named checks reobserved in current stable/MSRV CI. Historical sequencing and the
candidate-version order deviation remain explicit. Review coverage does not close
open qualification, performance or release gates or certify semantic completeness.

### Release acceptance review — November 16, 2026

This maps every [§14 condition](plan.md#14-release-definition-of-done-non-negotiable)
to its evidence and remaining obligation. “Implemented” describes the checked
capability, not final release qualification. Each linked artifact retains its own
source identity, checks and limits; full chaos, performance acceptance and final
release reconciliation remain open.

| Acceptance condition | Evidence and disposition |
| --- | --- |
| Unified programming model | Implemented: kernel/hierarchy checks, facade consumers and the 15-invariant audit cover typed dispatch, rejection, publication and provenance. Callback determinism remains a consumer obligation |
| No mandatory extras | Verified portable core bare/serde builds on both compilers; allocation probes cover allocation-free consumer types. The hosted umbrella facade is not the no-std target |
| Real integration | Implemented: prepared saga/task views and the protected Linux consumer exercise native admission, verified worker result, subsequent machine transition and saga admission. Native engines retain execution authority |
| No new rewind engine | Implemented: `MachineTimeline` delegates to the existing correction engine; frozen timeline observations and adapter tests pass. Timeline performance findings remain open |
| Durability exists | Implemented: journal/CAS, exact retries, snapshots, verified event replay, source-bound outbox, migration and eight kill boundaries have checked evidence. Full candidate chaos remains open |
| No effect guessing | Enforced in the audited native handoff, unknown-outcome and generation/reconciliation boundaries. Durable admission does not establish exactly-once external execution |
| Proven compatibility | Frozen observations, unchanged consumer, static signatures/export bindings and actual old-writer upgrade pass. Upgrade/backup-restore obligations are documented; unsupported downgrade is not offered |
| One public API | Canonical facade, migration table and source-derived first-machine tutorial are checked on stable/MSRV. Advanced durable metadata remains explicit; no local convenience layer is claimed |
| Reduced fragmentation | Shared owners and retained protocol boundaries are documented in ADR-050-06; inventories and rejected simplifications are retained. Production source grew with capabilities; exact clone counts do not prove absence of semantic duplication |
| Portable and bounded | Stable/MSRV RISC-V compilation, allocation probes, capacity/fault tests and invariant audit pass in their stated profiles. No physical-board, arbitrary callback or whole-process memory bound is inferred |
| Qualification complete | **Open:** immutable full chaos and performance acceptance. The later `7ab93d2` source/artifact reconciliation is complete within its recorded scope; seven green CI jobs and passing extracted packages do not close the remaining obligations |
| Release controlled | Candidate versions/order and unreleased notes are prepared. **Open:** final dated release entry and artifact reconciliation after qualification; tagging/publication still require separate authorization |

The §13.2 SQLite scope gap is now covered by the separate
[versioned 31-pair refresh](evidence/sqlite-balanced/versioned-candidate.json).
It confirms substantial legacy replay and exact-retry findings; database
performance acceptance remains open. The versioned capability archive does
record end-of-profile WAL/database sizes, race throughput and Linux process I/O
counters. All four writer profiles in all three stable runs and the MSRV run
retain a **3,827,512-byte WAL** and **4,096-byte main database** before close;
these are not peak size or post-checkpoint footprint measurements. I/O counter
deltas include warmup and verification, not per-commit physical device writes.
Race throughput covers barrier release through the last CAS result, not a general
application throughput bound. Earlier
empty `rlib_artifacts` inventories likewise supply no binary-size evidence;
the refreshed executable sizes apply only to the retained benchmark binaries.
Single-run maintenance test timings supply observations, not a flake-rate estimate.

## Performance evidence

Measurements retain their recorded revision scope. The new-capability refresh
uses the versioned candidate; legacy performance acceptance remains open.
Same-work comparisons keep original fixtures and independent locked builds.
CPU affinity, cache policy, shared-host interference and resource scope are
recorded; no isolated-host or worst-case latency guarantee is implied.

| Workload or finding | Retained evidence and disposition |
| --- | --- |
| Current-source legacy refresh | [31 pairs at `7ab93d2`](evidence/legacy-balanced/current-source.json): all eight unchanged workloads, 496 measured processes, 16 retained binaries and 1,000 timeline samples per profile/process. DTO encoding loop means are 302.60 → 349.27 ns (+15.42%, 31/31 pairs above 5%); one-word/one-step inherited reads are 210 → 290 ns p50 (+38.10%, 31/31 pairs) and 231 → 291 ns p95 (+25.97%, 30/31 pairs). Parallel graph-saga loop means flag at +7.15% (11/31 pairs above 5%). All 40 flags, including six handwritten controls, remain visible; none is accepted. Package tests on CPUs8–9 and coverage on CPUs10–11 may overlap serial CPU4/parallel CPUs4–7 measurements; shared-host interference remains uncontrolled |
| Current-source legacy SQLite | [Post-QPSG 31 pairs at `7ab93d2`](evidence/sqlite-balanced/post-qpsg-current.json): previous warm-replay and exact-retry threshold flags do not recur in this run. Small-history reopen p50 remains +10.66% and append p95 flags at +17.67%. Both remain open. Ten replay samples and fourteen small-history append samples per process do not establish tail bounds or equivalence; all earlier reports remain retained |
| Versioned candidate capabilities | [Frozen-source refresh](evidence/qualification/candidate-capability-performance.json) at `31c0cde`: seven existing harnesses, three stable runs and one MSRV run each, full 101-sample release profiles, all 14 executables retained. Flat/hierarchy/owned-state/timeline allocation probes pass within their recorded scope; SQLite races, bounded queues and real-worker recovery preserve their invariants. These observations do not accept legacy regressions |
| Current contention and recovery | In that refresh, the 32-writer race has a median run p50 last-finish time of **429.90 ms** and median run p99 of **1,429.94 ms**; the sequential protected recovery workflow has median run p50 **40.06 ms**. Writers that all see the old head can still queue. Earlier local preflight improvements do not establish a general latency improvement or bound |
| Versioned 0.5 versus 0.4 | [Complete 31-pair comparison](evidence/legacy-balanced/versioned-candidate.json): all 496 measured processes and 16 binary identities retained. DTO encoding loop means are 312.95 → 357.19 ns (+14.14%, 27/31 pairs over 5%); timeline retains 31 p50 and 9 p95 flags, including five handwritten Ring control flags. Saga/task loop-mean profiles do not cross the review threshold; that does not establish equivalence |
| Inherited reads in the versioned candidate | One-word/one-step inherited collection p50 is 211 → 291 ns (+37.91%); p95 is 231 → 301 ns (+30.30%), each crossing its threshold in 29/31 pairs. Additional inherited horizons/correction paths are flagged. Timeline production source and the shared core helper are unchanged since `94a7396`; this observation does not identify the cause or waive the new findings |
| Legacy saga/task/timeline | [Initial comparison](evidence/legacy-performance/manifest.json), [task/timeline extension](evidence/legacy-runtime/manifest.json), [31-pair follow-up](evidence/legacy-balanced/manifest.json): raw observations and all flags remain retained; process loop means and timeline p50/p95/p99 are distinct measurements |
| DTO encoding | The 31-pair development comparison measured 306.56 → 354.14 ns median **process loop means** (+15.52%, about 48 ns); this is not an individual-call percentile. [Layout controls](evidence/legacy-balanced/encoding-layout.json) reproduce sensitivity with identical text bytes; [inlining diagnostic](evidence/legacy-balanced/encoding-inline.json) rejects an unstable hint. No production hint or linker workaround is retained |
| Legacy timeline | [Later 31-pair comparison](evidence/legacy-balanced/timeline-current.json) retains history-capture p50 251 → 270 ns and history-append p95 291 → 321 ns flags. Its flagged `ring_replay` profile is a handwritten control, not a Penelope replay API call; it is neither subtracted nor used to waive library findings |
| Capture placement | [Focused placement experiment](evidence/legacy-balanced/timeline-capture-layout.json) reverses which revision is faster. Its filtered fixture is diagnostic, not a replacement for the frozen full benchmark; normalized assembly equality covers one helper, not the whole program |
| Legacy SQLite | [Balanced comparison and patched-dependency controls](evidence/sqlite-balanced/manifest.json) retain substantial replay/exact-retry costs. Patched original dependencies reproduce much of that cost; attribution does not accept it. No security downgrade or weaker validation is used |
| Versioned legacy SQLite | [Fresh 31-pair comparison](evidence/sqlite-balanced/versioned-candidate.json) against original 0.4 retains all 62 measured reports, both binaries and 16 p50/p95 flags. Warm replay median run p50 increases **165.39–168.44%** across 16/128/512 records, and exact process retry increases **39.87%**, each over 5% in all 31 pairs. Earlier dependency attribution keeps its original scope; no acceptance is implied |
| SQLite recompilation diagnostic | [CPU profiles, debugger counts and isolated query-plan stability probe](evidence/sqlite-balanced/reprepare-diagnostic.json) observe 27,157 internal recompilations in the candidate fixture versus 132 in original 0.4; the probe observes zero. Nine rotating triples reduce warm replay p50 about 62–64% versus candidate, but retain an append p95 flag and a small-history reopen flag versus original. At that diagnostic checkpoint the setting remained outside candidate code; the later adoption is recorded below and does not close acceptance |
| Broader query-plan probe | [SQLite tests and full storage/recovery profiles](evidence/sqlite-balanced/qpsg-qualification.json) pass on stable/MSRV, with formatting and strict Clippy. Three stable comparison pairs per fixture plus one MSRV run each retain all 21 reports and nine binaries. The 32-writer last-finish median is about 429 ms in both variants, but native total rises 40.05 → 42.54 ms (+6.23%); all 31 descriptive p50/p95 flags remain. Three pairs do not establish causal regressions or equivalence. At that broader-probe checkpoint the setting was still outside candidate code; see the later adoption below |
| Longer query-plan comparison and adoption | [31 legacy triples and 15 native pairs](evidence/sqlite-balanced/qpsg-balanced.json) retain all 123 measured reports, five whole-process warmups and five binaries. The replay gain repeats; native total p50 is 51.89 → 51.17 ms in this later shared-host run, without establishing equivalence. All reopen, append-tail and phase flags remain open. The small setting is now carried into candidate source for full qualification; this is not acceptance of the remaining performance findings. Earlier probe dispositions describe their original checkpoints |
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
| Maintenance | [Seven acceptance criteria](evidence/maintenance/acceptance-audit.json) map shared owners, retained boundaries and independent checks. [Inventory](evidence/maintenance/manifest.json): 14,356 → 18,873 potential production source lines at `cefa55f`, +107 across existing paths, two unchanged exact whole-function clone groups; 72 → 69 declared dependency edges and 171 → 59 tracked Markdown files. New capabilities grew production source; counts do not prove lower complexity or absence of semantic duplication |
| Rejected simplifications | [Task-reader experiment](evidence/maintenance/task-read-evaluation.json): fewer lines did not justify slower reads. [Inherited-prefix probe](evidence/maintenance/inherited-lookup-evaluation.json): independent models and negative controls pass, but faster inherited reads accompany a repeated root-collection regression. [Narrower fallback probe](evidence/maintenance/inherited-lookup-fallback-evaluation.json): retains the first validated hint and avoids that root slowdown, but inherited reads remain 291 ns versus the candidate's 291 ns. Both lookup probes remain outside candidate code. A [schema-open preflight probe](evidence/maintenance/open-preflight-evaluation.json) avoids the migration writer lock for current schemas and passes the focused lock control plus 59 SQLite unit tests, but seven shared-host timing pairs are too variable to establish a performance fix; it is also not adopted. All raw profiles and controls are retained |
| Earlier broad rehearsal | [Workspace qualification](evidence/qualification/manifest.json): earlier coverage, conformance, migration, consumer and supply-chain results retain their recorded source scope |

[Architecture owners and retained boundaries](architecture.md#adr-050-06-extraction-boundaries)
explain what is shared and why specialized task generations, saga compensation,
timeline finality and native transactions remain separate. Sharing inspection or
candidate vocabulary does not create universal execution authority.
The [15-invariant audit](evidence/maintenance/invariant-audit.json) binds each
plan §10 requirement to owners, source identities and passing negative/boundary
tests in the candidate archive. It records scoped evidence, not completion of
the remaining performance, campaign or release gates.
The [core/hierarchy/atomic-commit audit](evidence/maintenance/core-contract-audit.json)
maps all 26 explicit bullets in plan §4.1, §4.4 and §7.2 to source owners and
33 named tests observed passing on stable/MSRV CI at `9084239`. Its scope does
not include every remaining normative paragraph in the plan.

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
