# Initial stabilization and facade verification

This is an incremental verification ledger, not 0.5 release qualification.
Earlier sections describe their original checkpoint; later sections record the
machine kernel, hierarchy, shared admission, prepared adapters, scoped journal
and snapshots, native task/saga bridges and the verified result progression.
Complete release qualification remains open.
The [evidence manifest](evidence/manifest.json) contains compressed raw logs,
exit codes and hashes. Candidate source hashes identify the changed checkout.

## Baseline

Production sources and scripts matched the frozen 0.4 inventory before the
candidate changes were copied from an isolated checkout. Stable and Rust 1.85
workspace all-target/all-feature tests passed, as did strict rustdoc, script
checks and the signed StateChronicle conformance profile at its pinned upstream.
Coverage passed at 95.2713% against 94.0759%. Coverage here describes the original
production baseline, not new facade source coverage.

The default chaos runner was started with three rounds, 1,000 property cases and
10,000 requested fuzz executions per target. Seed one completed all 22 targets;
large existing seed corpora cause some targets to execute more than the requested
count. The campaign was deliberately terminated during round two to prioritize
the dependency fixes. Exit 143 is recorded; this is not a green three-round run.
The complete repeated release campaign remains required on the final candidate.

Release-mode executor `linear_saga`, `graph_saga` and `task_poll` benches passed.
Timeline `timeline_operations` passed with `PENELOPE_TIMELINE_SAMPLES=100`.
Other checks were running concurrently, so these raw measurements demonstrate
reproducibility and workload coverage, not isolated timing, stable tolerances or
an accepted regression budget. An initial unsupported 20-sample invocation was
refused by the benchmark's minimum-sample assertion; the accepted run preserves
that assertion and uses 100 samples.

Baseline audit and deny checks failed on RUSTSEC-2026-0459 (bundled SQLite) and
RUSTSEC-2026-0486 (rustix). Preserve these failures as baseline deviations.

## Initial facade/dependency candidate changes

The user explicitly approved Rust 1.89 as the new workspace minimum to consume
patched bundled SQLite. Workspace manifests inherit the minimum; CI installs
and tests it. The development toolchain was initially independently pinned to 1.99; the
subsequent Rust 1.100 checkpoint below updates that pin.

The workspace now resolves `rusqlite` 0.41.1, `libsqlite3-sys` 0.39.1 and `rustix`
1.1.6. SQLite version selection is centralized. The conformance lockfile and its
generated manifest also select patched SQLite. Audit and deny checks pass.
No advisory is suppressed, and no dependency patch is maintained locally.

The facade now offers `saga`, `storage`, `runtime`, `integration::statechronicle`
and hosted supervision under `task::TaskSupervisor`. The saga export list has one
owner; root compatibility exports reuse it. Existing protocol-specific types and
implementations retain their identity. The trade example uses canonical saga
imports. These paths do not implement the proposed application-machine model.

## Candidate checks

- Workspace all-feature tests and doctests on Rust 1.99 passed after dependency
  and facade changes. Subsequent Rust 1.89 SQLite/runtime lib and integration
  tests passed after the mechanical Clippy simplifications.
- Final Rust 1.89 workspace library/integration tests passed after all code
  changes; final signed conformance passed again on the branch checkout.
- Strict workspace all-target/all-feature Clippy passed on the final code.
- Strict facade rustdoc passed; full baseline workspace docs are separate evidence.
- All seven signed conformance cases passed against patched dependencies.
- Rust 1.89 RISC-V core default/Serde and timeline default/alloc checks passed.
- The frozen legacy consumer compiled with default and timeline features on
  Rust 1.89; the canonical trade example ran successfully.
- Formatting, whitespace, action pin and dependency-boundary checks passed.
- The graph was updated after the final source changes.

The imported plan's complete F0 and final release gates remain open: extend the
consumer fixture to frozen signature/exhaustive-match and cross-version history
fixtures; complete repeated campaigns on the final candidate; establish isolated
same-work performance tolerances; verify packages and remaining release gates.
Do not infer a new wire format, SQLite migration or durable-machine guarantee
from the namespace changes.

## Portable machine checkpoint

The initial kernel and hierarchy implementation is described in [machine.md](machine.md).
Raw logs and a source hash manifest are under
[evidence/machine-checkpoint](evidence/machine-checkpoint/). These supplement,
rather than replace, the earlier baseline and dependency checkpoint evidence.

- Strict workspace Clippy with all features/targets: passed.
- Workspace library and integration tests, all features: passed.
- Rust 1.89 machine/hierarchy tests, no-default allocation probe and core
  compile-fail documentation: passed.
- Rust 1.89 bare-metal RISC-V core builds, default-free and serde: passed.
- Independent facade-only flat and nested consumers on Rust 1.89: passed.
- Instrumented `fuzz_machine` and expanded `fuzz_newtypes`: 10,000 runs each,
  seed 501, passed. This is bounded smoke evidence, not the full chaos campaign.
- Runner regression script: passed with the new core tests and fuzz target.

F2 remains in progress. Full release coverage, chaos, performance qualification,
specialized adapters and durable machine protocols remain outstanding.

## Shared staging extraction

[The extraction record](staging-extraction.md) documents the shared owner,
retained domain rules, differential oracle and alternating parent/candidate
performance comparison. Reproduction script, source and binary hashes, raw
p50/p95/p99 measurements and verification logs are preserved under
[evidence/staging-extraction](evidence/staging-extraction/).

Workspace library/integration tests and documentation, strict all-target Clippy,
Rust 1.89 machine tests/allocation probe, portable core/timeline builds and the
layer gate regression suite passed. Machine, timeline effects, branch promotion
and correction fuzzers each passed 10,000 instrumented runs with seed 502.
Allocation/memory benchmark records are identical to the parent. This checkpoint
consolidates metadata admission only; specialized engine adapters and full release
qualification remain pending.

## Prepared saga/task adapter checkpoint

[The adapter record](prepared-adapters.md) and [implemented public API](public-api.md)
record the shared inspection interface and distinct commit authorities. Logs,
checkpoint source hashes and full parent/candidate RISC-V profile bundles are in
[evidence/prepared-adapters](evidence/prepared-adapters/). The independently compiled
0.4 saga observations and their source/output hashes are in `ci/saga-parity/baseline`.

Workspace library/integration tests, documentation and strict all-target Clippy
passed. Rust 1.89 passed the new task parity tests, existing zero-allocation
probes, bare-metal core/timeline builds, facade example consumers and three-way
saga parity. Trade, compensation and timer examples passed through preparation.
Task-history and linear-engine instrumented fuzzers passed at least 10,000 runs
with seed 503 (the linear corpus initialization executed 55,967 inputs).
The fixed-port RISC-V stack bound remains 115,056 bytes, with the independent
65,552-byte positive-control branch accounted for. Full latency and release
qualification, runtime effect bridges and machine durability remain outstanding.

## Completed unchanged 0.4 chaos campaign

The full default campaign subsequently passed on the clean original
`42ec187c9adfc7403b06215f405f2575f53c35c2` checkout: three rounds/seeds, 1,000
property cases and all 22 fuzz targets per round with at least 10,000 executions
each. Exit zero, raw output, toolchains, lock hash and all 66 target-run counts are
preserved in [baseline-complete-chaos](evidence/baseline-complete-chaos/).
This supersedes the earlier interrupted baseline attempt; that attempt remains
recorded rather than being relabeled successful. These original-source results
do not qualify the final 0.5 candidate or erase the baseline advisories. Bench
output produced by all-target tests is not isolated performance evidence.

## Optional machine/timeline adapter checkpoint

[The composition record](timeline-machines.md) defines captured context, separate
native/timeline coordinates, snapshot obligations, failure behavior and the
existing finality boundary. No correction or publication engine was duplicated.
The flat and timeline facade examples share one pure counter model.

Workspace library/integration tests, doctests, strict all-target Clippy and strict
facade/timeline rustdoc passed. Rust 1.89 passed timeline tests, the final seven
adapter tests, independent facade examples, legacy compatibility and default-free
bare-metal RISC-V compilation. The adapter tests include 2,048 native differential
event attempts, actual `FixedHistory` restore/replay, non-Clone state preparation,
captured-context hashing, hierarchy, panic poisoning and finalized-prefix refusal.
The machine, correction, effects and branch-promotion fuzz targets each passed
10,000 instrumented executions with seed 504. The machine target now includes an
independent vector oracle for the adapter at capacities zero, one and four.

Raw logs, commands, toolchains and checkpoint source hashes are in
[evidence/timeline-machines](evidence/timeline-machines/). The manifest records
test timing relative to the final adapter-test additions. The layer gate and its
regression suite passed, and the code graph was updated. Full F5/F8 qualification,
new-adapter resource/performance measurements, machine durability and cross-engine
admitted effects remain open.

## Independent machine commit metadata checkpoint

[The machine wire record](machine-wire.md) fixes the initial v1 accepted-batch
metadata layout independently of the old protocols. Frozen fixtures use a
separate Python `struct`/SHA-256 encoder; Rust verifies exact bytes, commit
identities, every truncation and byte mutation, rehashed structural violations,
intent/revision/charge bounds and untouched short output buffers. Fixed-field
read/write cursors are shared with timeline while its seven goldens stay unchanged.

Workspace library/integration tests, strict all-target Clippy, workspace doctests
and strict core rustdoc passed. Rust 1.89 passed new machine and existing timeline
wire tests, the allocation probe, legacy facade compatibility and a default-free
bare-metal RISC-V build. The allocator's positive control passed; 10,000 machine
wire decode/construct/encode/refusal cycles observed no allocations. Machine,
timeline-wire and newtype instrumented fuzz targets each passed 10,000 executions
with seed 505. The architecture gate initially identified five missing direct
newtype fuzz calls; the calls were added before the recorded successful rerun.
Layer/runner regression gates passed and the code graph was updated.

Commands, raw logs, toolchains, parent revision and exact changed/new source hashes
are in [evidence/machine-wire](evidence/machine-wire/). This supplies metadata
encoding evidence, not crash durability or admission authority. Payload codecs,
accepted-candidate durable preparation, store/receipt contracts, SQLite migration,
crash/reopen checks and downstream effect bridges remain pending, as do the full
F1/F6/F8 gates and final performance qualification.


## Initial machine journal and SQLite checkpoint

The [storage contract](machine-storage.md) records the implemented scope and
limits. This checkpoint adds host-context authorization, canonical genesis,
coherent snapshots, exact-event lookup, atomic accepted batches and bounded
history. It reuses the bounded SQLite worker, connection setup, checked SQL
writes and column reads. Stored-commit and head-snapshot restoration now share
one canonical helper; no second model evaluator was added. Legacy migration
fixtures remove the new tables when synthesizing old schemas; their existing
row/fence, page-limit and rollback assertions remain intact at schema version 7.

[Evidence](evidence/machine-storage/manifest.json) pins changed/new source bytes
and compressed verification logs. New tests cover actual competing workers,
concurrent readers, unauthorized reads/retries, corrupted bytes, quotas and
silent-write rollback. SIGKILL commit/migration tests verify coherent reopen and
exact retry. The independent facade/adapter counter example runs on stable and
Rust 1.89. The new real-SQLite fuzzer and deterministic deep seeds extend the
bounded runner; final candidate coverage, full repeated campaigns and isolated
performance qualification remain open. This is not proof of completed outbox
reconciliation, typed subsystem bridges, F6 or F8.

## Rust 1.100 toolchain checkpoint

The development toolchain is pinned to Rust 1.100.0 (released November 12,
2026). CI conformance and the measurement/verification scripts use that same
version. The previously approved Rust 1.89 library minimum is retained.
Historical measurement evidence keeps its original compiler identity.

New Cargo diagnostics exposed unused internal dependency edges and redundant
manifest metadata; these were removed and the exact dependency layer gate was
narrowed accordingly. The fuzz package explicitly retains its existing lint
profile. New Clippy diagnostics were resolved by removing unnecessary fixture
clones while preserving the clone-contract assertions and corruption checks.
No library behavior or wire format changed.

Workspace library/integration tests, strict Clippy, rustdoc/doctests, all 66
script tests, dependency-layer gates, the frozen legacy consumer, the durable
machine consumer and RISC-V core/timeline builds passed on Rust 1.100. The
seven signed-source conformance cases also passed. Rust 1.89 workspace checks
and the durable consumer passed. Formatting and diff checks passed.

Results and source hashes are recorded in
[`evidence/rust-toolchain/manifest.json`](evidence/rust-toolchain/manifest.json).
These checks cover the toolchain update; remaining release qualification in
the plan is still required.

## Machine intent outbox checkpoint

The [leased handoff contract](machine-storage.md#leased-receiver-handoff) adds
an optional journal view with host-pinned receiver verification, a distinct trusted
lease clock, persisted attempt uncertainty and exact source/lease fencing. Native
receipt verification runs outside the database worker, with transactional checks
before and after it; no generic external-effect executor or restart path was added.
The configured native receipt type remains intact at the acknowledgement boundary.

Version 8 preserves all machine event wire bytes and adds bounded delivery state
plus clock/usage columns. Delivery metadata and journal bytes share the scope
budget. Existing process/timer leasing and machine leasing now use one extracted
checked durable counter, including post-write verification. No legacy token type
or protocol encoding changed.

Focused integration tests cover normal/uncertain/reopened delivery, independent
workers, byte ordering, whole-prefix quota rollback, denied operations, malformed
or substituted coordinates/receipts, delayed verification, lost ACKs, time
regression, attempt/token exhaustion and ignored/substituted/deferred writes.
Actual SIGKILL tests cover eleven claim/ack transaction boundaries and five
populated version-7 migration boundaries. Existing schema migration, row/fence,
page-limit, corruption and rollback assertions remain in place at version 8.

[Checkpoint evidence](evidence/machine-outbox/manifest.json) records the exact
source hashes and verification commands/results, including a real-SQLite fuzz
oracle for lease/expiry/acceptance operation sequences and deterministic deep
seeds. This checkpoint qualifies the generic source-bound handoff mechanism. It
does not prove production machine-to-saga/task bridges, downstream effect
reconciliation, final performance/coverage campaigns, or completion of F6/F8.

For this checkpoint, workspace library/integration tests, strict Clippy, rustdoc
and doctests, all 66 script tests, layer gates, frozen legacy consumer, durable
consumer and all seven signed-source conformance cases passed. Rust 1.89 passed
all 13 focused outbox integration tests. The final-source outbox campaign
completed 10,000 cases with seed 512; existing newtype, process and journal fuzz
checks also passed. The manifest records actual completed counts, including
initial corpus processing. The final outbox corpus is retained alongside the logs.

## Native machine-to-task admission checkpoint

The production `MachineTaskReceiver` now admits source-authorized machine intents
through the existing native task projection and journal. Complete source and
receiver-policy identities bind the native task. Native receipt verification
reads the exact record and payload; outbox acknowledgement remains a separate
source transaction. Redelivery reconstructs the initial intent, including its
original tick, without driving a supervisor or adding a generation.

The SQLite async task view owns the unchanged synchronous journal on the existing
bounded worker. This shares queue, cancellation, wakeup and panic containment
rather than implementing another task storage path. Tests cover buffer integrity,
queue saturation and commit completion after its response future is dropped.

[Source hashes and raw checkpoint logs](evidence/machine-task/manifest.json)
cover the full workspace, strict Clippy/rustdoc, Rust 1.89, independent facade
consumer, frozen legacy imports, 3,136 saga parity observations, 66 script tests,
layer gates and seven signed-source conformance tests. Focused handoff checks
cover competing native writers, lost native/source acknowledgements, corrupted
input, forged receipts, receiver policy denial, scope quota and redelivery of
running/faulted tasks including reconciliation-required tasks. Ten actual SIGKILL
cases exercise five handoff cuts in shared and separate databases. Independent
SHA-256 source-identity and bounded real-SQLite fuzz oracles supplement these
checks; the evidence manifest records the final campaign and retained corpora.

This checkpoint proves task admission and recovery boundaries. Production saga
composition, final performance/coverage campaigns and complete F0–F8 release
qualification remain open. No successful external effect or automatic retry is
inferred from an admission receipt.

## Native machine-to-saga admission checkpoint

`MachineSagaReceiver` admits a source-authorized intent through the existing
linear or graph engine and native atomic process commit. The receiver pins the
workflow, principal and policy identity. Native start authorization precedes
writes; source context and unchanged native replay events are retained separately.
Redelivery recovers the original start time and batch, including after later
escalation. Admission dispatches no effect and grants no retry authority.

The optional `ProcessCommitVerifier` authenticates the complete retained native
batch through the same verifier used by commit retries. Read-only tests reject
missing commit markers or actions and install write-aborting triggers to prove
that receipt lookup cannot repair history. The original `ProcessStore` contract
remains unchanged.

First source claims now persist their receiver identity. Reopening with a changed
receiver cannot admit the intent again after a lost source acknowledgement.
Schema 9 preserves accepted receipts; pending version-8 deliveries with unknown
receiver identity require reviewed recovery and cannot be automatically retargeted.
Migration crash tests preserve rows and uncertainty across transaction cuts.

[Source hashes, logs and retained fuzz corpora](evidence/machine-saga/manifest.json)
record workspace tests, strict Clippy/rustdoc, Rust 1.89, independent consumers,
frozen legacy imports, 3,136 parity observations, 66 script tests and seven
signed-source conformance cases. Focused saga tests cover source/native denial,
racing starts, lost acknowledgements, read-only complete-batch verification,
advanced histories and 20 SIGKILL cases across shared/separate databases and
linear/graph engines. Final-source saga and outbox fuzz campaigns each completed
10,000 cases (seeds 518 and 519).

This checkpoint completes the production admission bridges. Final performance,
coverage and complete F0–F8 release qualification remain open.

## Consumer recovery progression

The canonical durable consumer now rejects an invalid overflowing transition,
checks the committed intent's source event/commit/revision, and reopens both source
and task adapters between native admission and source acknowledgement. Expiry
permits reclaim; native lookup returns the exact original task, the stale lease
is refused, and the new lease acknowledges it. The example then admits a saga
from a second event and shares the monotonic host clock across all adapters.
This is an explicit reopen demonstration; actual process-death tests remain in
the preceding admission checkpoints.

The unchanged full-source binding implementation moved from the specialized task
module to receiver delivery. Task and saga identities retain one authoritative
implementation and identical bytes. No evaluator or storage algorithm changed.
[Focused checks and source hashes](evidence/coherent-consumer/manifest.json) cover
source-coordinate tests, both bridge suites, the consumer on Rust 1.100/1.89 and
strict workspace/consumer Clippy. This improves onboarding but does not complete
the plan's verified effect-result round trip or final release qualification.

## Verified result returning to the application machine

The optional `protected` mode extends the same canonical durable consumer with
the existing native supervisor, SQLite generation authority and Linux runtime.
After uncertain source delivery is recovered and acknowledged, it executes the
admitted pure task, reconstructs its completed projection from native history,
reads and verifies exact output bytes/schema and checks application meaning.
Only then does it prepare and commit the next counter event. The stable result
identity binds native task identity, generation, output sequence and digest; the
commit retains task/source causation. An exact retry returns `AlreadyCommitted`.
That transition's intent starts the native saga. No direct state mutation or
second task evaluator is introduced.

The existing supervised-saga script now runs this progression and a negative
worker whose completed output has the wrong application meaning. The negative
case must fail before the machine result event is admitted. Stable and Rust 1.89
CI run the same script. Default admission still omits the protected runtime from
its dependency graph, and portable/pure machine profiles are unchanged.

[Source hashes and logs](evidence/machine-workflow/manifest.json) cover both
toolchains, successful and refused results, default admission, strict external
consumer Clippy, layer checks, 66 script tests and the default dependency graph.
This is a local, host-authenticated example with a pinned pure worker and fixed
application schema. External-effect success would need independent evidence.
The saga's action remains admitted rather than executed in the machine leg;
complete release performance/coverage and F0–F8 qualification remain open.

## Task/timeline compatibility and current-toolchain coverage

The frozen legacy import source remains unchanged. An additional independent
consumer now compiles selected public function signatures and exhaustive matches
for task status/events and timeline record variants. Its 33,278 expected byte,
error and replay observations were generated against the untouched 0.4 checkout
at `42ec187c9adfc7403b06215f405f2575f53c35c2`, then reproduced on its original
Rust 1.85 minimum. The current branch matches on Rust 1.100 and Rust 1.89.
All recovery/effect policies and task event variants are represented; timeline
records include strict prefixes/trailing bytes and checkpoint restoration.
The [producer manifest](../../ci/compat-0.4/baseline/manifest.json) retains its
lockfile, source and output identities. The existing parity command enforces
this corpus alongside the unchanged 3,136-observation saga corpus.

The first current-toolchain coverage attempt reported 84.2632%. That report mixed
inactive binaries from the older compiler layout with current artifacts. Fresh
separate directories measured 94.5527% on Rust 1.99 and 93.4268% on Rust 1.100
for the same production sources. Workspace-only cleanup did not remove the old
layout. The coverage runner now cleans its complete instrumented build directory
before measuring; ordinary non-coverage builds stay cached.

Actual public-contract coverage gaps were addressed with proposal/abort tests
through canonical facade paths, malformed native acknowledgements and receiver
proof failures, corrupt delivery rows/orphaned sources, and signed/byte-array
history payload admission and duplicate accounting. These checks preserve
uncertainty, reject source acknowledgement before exact native proof and verify
unchanged journal state on failure. No production behavior or protocol was changed.

The final clean Rust 1.100 workspace coverage run passes at **94.0994%**, against
the unchanged **94.0759%** ratchet.
[Coverage reports, failed-attempt context, raw logs and source hashes](evidence/compatibility-coverage/manifest.json)
retain the measurement scope. Strict workspace/fixture Clippy, focused Rust 1.89
tests, both parity consumers, layer gates and 66 script tests also pass. This
closes these selected compatibility/coverage gaps; full performance, packaging
and final release qualification remain open.

## Archive verification and legacy performance checkpoint

All ten library archives pass the existing packaging script, which verifies
unpacked libraries against a fresh temporary registry. The
[archive manifest](evidence/packaging/manifest.json) binds each archive to
`18f99c0`, records its digest and preserves the complete log. Versions remain
0.4.0 during development. This does not qualify the final 0.5.0 candidate or
authorize publication. Static inspection also found 14 workspace-relative
test/bench includes absent from individual archives; library compilation does
not prove standalone archive testing, all-feature or MSRV behavior.

The six unchanged original saga/contract benchmarks were built from the pinned
0.4 baseline and `18f99c0` using Rust 1.100, separate locked build directories
and pinned CPUs. Five alternating measurements put serial linear saga at
-6.57%, graph at +0.50%, replay at +1.86%, parallel graph at -4.74%, DTO encoding
at -0.31% and bounded outcome append at -0.86% median time per operation.
Parallel linear initially regressed +19.27%; a separate nine-run comparison
measured -1.31%. Both results and their wide ranges remain in the
[raw performance evidence](evidence/legacy-performance/manifest.json).

The parallel fixtures run for only tens of milliseconds on this shared host.
The initial slowdown was not sustained, but these samples cannot establish
statistical equivalence or operation latency percentiles. No benchmark sources
or operation counts were changed. Task, timeline, SQLite, machine and complete
workflow measurements, maintenance accounting and exact-candidate qualification
remain open.

## Shared fixtures in independent archives

The 14 missing literal test/bench includes above now resolve through eight
package-local links to the existing authoritative fixtures. Cargo materializes
their contents as ordinary archive files; no production helper or duplicate
fixture implementation was added. Repository checkouts must preserve Git
symlinks; consumers of the published archives do not need symlink support.
The packaging script now rejects missing literal Rust includes before retaining
archives, with regression cases for missing neighbors, assets and path escapes.

All ten rebuilt archives pass library verification and the include check.
Extracted archives, patched only to resolve their candidate sibling dependencies,
compile all targets/features on Rust 1.100 and 1.89. Their native task/saga bridge
tests and SQLite library tests also run successfully outside the checkout.
Workspace strict Clippy, focused tests, formatting and 70 script tests pass.
[Raw logs, archive/source identities and the isolated workspace lock](evidence/package-fixtures/manifest.json)
retain the exact scope. Full archive test execution is not claimed: repository
architecture tests intentionally inspect the workspace and fuzz harnesses.
Final candidate versions, remaining performance and release qualification stay open.

## Actual 0.4 SQLite writer upgrade and backup restore

The new independent SQLite consumer compiles identical producer/reader source
against the untouched `42ec187` checkout and the current adapter. Its old build
uses only the original baseline dependency versions/checksums; the external
consumer itself is the sole added lockfile package. The original writer creates
schema 6 with task input/history/start, a consumed process input, outcome and
commit, immutable process payload and a claimed unacknowledged external action.
No worker or external effect runs.

The frozen old database and expected public replay are retained independently
of the candidate. On Rust 1.100 and 1.89, the current adapter upgrades it to
schema 9, reopens again, preserves every row and table schema in all 15 legacy
tables, reproduces exact task/process replay and payloads, and passes integrity
checks. Storage types are compared along with values. Current/MSRV CI now run
the same check. Actual original task/process adapters refuse the upgraded copy
without changing its rows/version; separately restoring the original backup
lets both old adapters replay it again.

[Producer/database identities](../../ci/sqlite-upgrade/baseline/manifest.json),
the frozen lock and [raw qualification logs](evidence/sqlite-upgrade/manifest.json)
record this bounded case. Strict consumer Clippy, formatting, action pin checks
and 72 script tests pass. This adds real cross-version evidence to the synthetic
migration/failure suites; it does not qualify every existing deployment or
authorize rolling back accepted external effects. The compatibility matrix now
states the quiesced upgrade and separate-backup rollback procedure.

## Maintenance and qualification cost inventory

The reproducible [inventory and raw measurements](evidence/maintenance/manifest.json)
compare the original `42ec187` tree with `20941ba`. Library count stays at ten;
declared normal dependency edges stay at 52, including optional edges. Three
unused internal edges were removed while timeline hashing and native bridge
dependencies were added. This is a declared graph inventory, not a feature-specific
resolved dependency count. Tracked Markdown files fall from 171 to 59.

Crate `src/` text grows from 26,673 to 32,483 physical lines: 5,646 lines in new
files and a net 164 in existing paths. These counts include comments and inline
test code, so they are not production-only semantic LoC. Overall Rust text has
grown to implement the new machine/durability/integration contracts; no overall
code-size reduction is claimed. The architecture audit identifies actual shared
owners and explains retained native protocol/authority boundaries rather than
forcing incompatible engines into one evaluator.

Prebuilt, all-feature library/integration suites pass 572 baseline and 685 current
tests, with zero failures or ignored tests. Single-run wall time is 30.32 versus
38.41 seconds; the test sets differ and this is qualification cost, not a library
throughput comparison. One successful run cannot establish a flake rate.

Fresh-target release builds of the default facade, Rust 1.100, four pinned CPUs
and four Cargo jobs take 5.47 versus 5.17 seconds after explicitly disabling both
compiler wrappers. Initial cache-assisted runs are retained separately and do
not support a cold-build claim. Registry/toolchain/OS caches remain warm; this
is an artifact-cold compilation measurement, not a fresh install or a speedup
claim. The remaining workload performance matrix and final exact-candidate
qualification are still open.

## Legacy task polling and timeline performance review

The existing comparison runner now accepts the unchanged task-poll and timeline
operation fixtures while retaining its original six-workload default. Source
comparison includes their shared fixture files. Timeline CSV parsing preserves
its per-fixture p50/p95/p99 values and sample counts; aggregate task means stay
separate. Parser tests reject missing, malformed and unordered measurements.
All 75 script tests pass.

[Both raw comparisons and every profile summary](evidence/legacy-runtime/manifest.json)
compare `42ec187` and `5281290` with Rust 1.100, CPU 4, independent locked builds
and alternating order. Each run matches nine task and 640 timeline profile
identities and operation/sample counts. The initial five rounds put the largest
hosted pending-task profile at +9.30%; nine subsequent rounds put it at -9.08%.
All task profiles are within +5% or faster in the repeat. These are aggregate
polling measurements, not startup/recovery or per-operation tail qualification.

Timeline threshold exceedances fall from 68 to 21 for median p50 and 108 to 36
for median p95. Eleven p50 and thirteen p95 profiles exceed the provisional
thresholds in both comparisons, including history collection and full correction
with eviction. Those remain review blockers. Some handwritten controls also
vary, but their timings are not subtracted and do not excuse a sustained library
regression. No workload, threshold or production behavior was changed to obtain
a passing result. Diagnosis, remaining performance profiles and final release
qualification remain open.

## Validated history hints after eviction

Sparse retained histories were missing the original floor-relative input hint
and scanning slots left empty by eviction. That path existed in both versions.
A second, wrapped-position hint now runs only after the original hint misses
and the floor has moved. Both candidates must pass the existing full-record or
canonical/branch check, followed by payload digest and byte-charge verification.
The original ancestry/override precedence and full scan remain the fallback.
No history is compacted or rewritten and no mutable index or allocation is added.

The first eager two-hint implementation improved eviction but slowed inherited
reads; the retained lazy version preserves the original successful-hint path.
[Both comparisons and all profile results](evidence/timeline-lookup/manifest.json)
remain available. Against the unchanged 0.4 benchmark on Rust 1.100, five
alternating rounds measure evicted collection p50 improvements of 68–74% for
horizons 1/8/32/128, and full correction improvements of 22–63%. Horizon 512 stays
near baseline. Inherited-read median changes range from -1.21% to +0.30% across
all five horizons. These are local run-level observations, not statistical proof.

Timeline suites pass on Rust 1.100 and 1.89. Ten thousand seeded independent
lookup-oracle cases, explicit post-eviction corruption checks, strict Clippy,
10,000 instrumented correction/lookup fuzz runs and both MSRV bare-metal timeline
profiles pass. All 643 allocation/size records exactly match the actual 0.4
baseline. The first cache-dependent checks were deliberately stopped after
stalling; isolated wrapper-free replacements provide the passing evidence.

The full comparison still contains 59 p50 and 27 p95 threshold exceedances across
640 profiles. They are retained for review; this targeted improvement does not
accept those results or close the wider performance, coverage, chaos or final
release qualification gates.

[Descriptive triage](evidence/timeline-lookup/lazy-triage.json.gz) retains every
flag and its five paired values. None exceeds its threshold in all five pairs,
which does not establish equivalence. The largest absolute differences cluster
in narrow-state indexed replay, whose fixture and replay/publication source files
match 0.4 exactly. Those selected paths use fixture-owned stores and stage no
effects, so they do not invoke the changed history hints or intent preflight.
Generated code, dependencies and runtime conditions can still differ. All flags
remain open pending balanced repeat measurements after chaos finishes.

## Flat machine measurement fixture

`cargo bench --locked -p penelope-core --bench machine_operations` now exercises
396 flat-machine profiles: 2/8/32 states and events, 8 B/256 B/4 KiB/64 KiB owned
state, guards, 0/1/8/32 intents and count-quota refusal. It uses the public portable
API and existing isolated allocation instrumentation, with no new dependencies.
Each profile checks disposition, whole state, revision and intent payloads outside
timing; quota refusal must preserve state and head. Allocation counting has a
positive control and runs separately from latency sampling.

[Raw samples, source hashes and scope](evidence/machine-performance/manifest.json)
record three Rust 1.100 runs pinned to CPU 4 and one unpinned MSRV 1.89 compatibility
run. All profiles pass with zero allocations, reallocations, deallocations and
requested bytes. Strict Clippy and packaged literal includes also pass. Quantiles
are explicitly **32-operation batch averages**, not individual-operation tail
latency; throughput uses total sampled time. Context construction, consumer linear
dispatch/state copy, publication and intent drain are included, with capacity 32
throughout. Rejected profiles return before intent staging.

These short fixed-order shared-host runs establish a reproducible new-machine
fixture, without a historical equivalent or a regression conclusion. Separate
consumer-computation measurements, hierarchy, durable SQLite, end-to-end workloads
and final candidate qualification remain open.

## Hierarchical machine depth measurements

`cargo bench --locked -p penelope-core --bench machine_hierarchy` adds 60 profiles
at depths 1/2/4/8/16, handled at leaf/parent/root, for internal transitions,
external sibling transitions, rejection and unhandled events. Independent
closed-form expectations check final state, revision and exit/entry/action
counter totals. At depth 1 the external transition is a self transition and
all handler labels coincide; at depth 2 parent and root coincide.

[Three pinned runs, MSRV results and raw samples](evidence/machine-performance/hierarchy.json)
pass all hierarchy profiles and requalify all 396 flat profiles after extracting
their shared sampler. Allocation probes remain zero with a positive control;
strict Clippy and packaged includes pass. Sibling-transition median batch averages
are approximately 50 ns at depth 1 and 240 ns at depth 16 on this host. Prefix
cycle checks are bounded by active depth and can be quadratic; no linear-scaling
claim is made. Inline owner size excludes transient path workspace and is not a
peak-stack measurement.

Hierarchy timings include context construction, status checks, state reads, drain
and counter-only hooks, with intent capacity zero. They are not directly compared
to flat profiles with capacity 32. Consumer-computation separation, durable and
end-to-end workloads, statistical review of legacy findings and final release
qualification remain open.

## Workspace qualification rehearsal after timeline lookup changes

A clean sequential Rust 1.100 coverage run passes at **94.105343%** (14,847 of
15,777 lines), above the unchanged 94.0759% ratchet. The 75 Python script tests,
action pins, layer boundaries and negative tests, independent machine wire
fixtures, chaos-runner target/seed checks and formatting pass. Documentation
cleanup removes stale claims that the implemented native bridges and durable
outbox are still absent; all 46 local file links in the edited guides resolve.

[Reports, source identities and failed attempts](evidence/qualification/manifest.json)
retain the full scope. Initial parallel coverage, workspace, MSRV and chaos
attempts were inconclusive during disk-quota exhaustion: MSRV compilation and
LLVM profile writes explicitly failed for lack of space, the workspace log
truncated, and the chaos workload returned `Unavailable` before fuzzing. No
passing campaign is inferred from those failures. All handles were confirmed
terminal before removing their new temporary build directories; sequential
coverage then passed. The stable all-target/all-feature workspace retry also
passes, including the previously failing application workload and all 67 protected
runtime tests. The full MSRV 1.89 all-target/all-feature workspace retry also passes,
as do workspace doctests, strict Clippy, warning-free API docs, dependency audit
and advisory/license/source policy checks. The existing `hashbrown` duplication
warning is retained. The secret scanner reports no findings across 888 inspected
commits, and all 216 local Markdown file references resolve. Debug benchmark
execution is not performance acceptance. All four core/timeline bare-metal
profiles also pass on Rust 1.89. The full documented chaos campaign has restarted
with three rounds, 10,000 property cases and 100,000 fuzz executions per target;
completion is not claimed. Remaining performance findings and exact 0.5 candidate
qualification remain open.

The pinned StateChronicle conformance rerun passes all seven selected signed-source
and checkpoint-proof tests, including real workers and SIGKILL recovery. Its
upstream tracked tree is clean at `ae3d94bc22c7835eaa3fdcd766e3c117929c12bb`;
the remaining 25 tests are deliberately outside this selected profile. The
qualification manifest retains the exact command, lock identity and complete log.

The README installation and first-machine tutorial also pass in a fresh external
application with the actual Git branch dependency, without workspace patches or
copied libraries. The exact Rust code fence prints `count = 3` on Rust 1.100 and
1.89 using one resolved lockfile. The qualification manifest records the fetched
Git identity, compiled source digest, lockfile and complete log. This checks the
default hosted onboarding path; optional deployment profiles remain separate.

The complete external consumer matrix also passes on Rust 1.100 and 1.89:
flat/nested/prepared facade examples, optional timeline and durable consumers,
and the protected-worker progression with wrong-result refusal. Legacy import
fixtures compile with default and all features. Frozen parity retains all 33,278
task/timeline and 3,136 saga observations through both saga facade paths, without
regenerating expectations. The original SQLite writer fixture upgrades and
reopens twice on each toolchain with exact rows, schemas, replay and integrity
across all 15 legacy tables. The qualification manifest retains every run log
and all four consumer lock identities. Full chaos and final candidate gates
remain open.

The existing SQLite measurement tool now accepts an explicit `--repository` and
`--toolchain`, allowing original and current checkouts to use the same compiler
and data filesystem without copying workloads or changing the baseline tree.
The durable workload remains byte-identical to 0.4. Runner identity and both
compiler-wrapper settings are recorded. Syntax/help checks and all 75 script
tests pass; this tool change supplies no performance conclusion.

During round-one SQLite fuzzing, the verified live campaign and its child threads
were pinned to CPUs 8–15. Workloads, seeds and execution counts are unchanged.
Performance runs can use CPUs 4–7 with separate SMT siblings, while unrelated
applications, frequency, thermal state and OS caches remain uncontrolled. The
qualification manifest records the affinity change and its timing.

## Balanced legacy performance follow-up

The [complete 31-pair comparison](evidence/legacy-balanced/manifest.json) uses
unchanged fixtures, Rust 1.100, alternating revision order and pinned CPUs. All
496 measured processes completed; the timeline matrix retains all 640 profiles
at 1,000 samples each. Saga/task loop means remained below the existing review
threshold. Canonical DTO encoding was 15.52% slower at the median and crossed the
5% threshold in 30/31 pairs; this remains an unresolved review blocker. Timeline
retains nine p50 and two p95 threshold flags. These results do not establish
statistical equivalence or accept any retained regression. Raw observations,
resource records, artifact/source hashes and exploratory intervals are preserved.
The full chaos campaign and exact-candidate qualification remain open.

The [legacy SQLite comparison](evidence/sqlite-balanced/manifest.json) completed
nine alternating pairs on NVMe Btrfs with WAL/FULL unchanged. A further nine
rotated triples compare original source/dependencies, original source with the
patched dependency stack, and current source. The patched original reproduces
most of the substantial replay and exact-retry cost; this distinguishes dependency
cost from refactor cost without accepting either. All raw runs, setup failures,
source/dependency identities and per-profile flags remain available. The canonical
DTO diagnostic instead leaves patched-original encoding close to original and
current encoding slower, with near-identical whole-process instruction counts;
its cause remains unresolved. No security downgrade or weaker validation was used.

## Owned-state and consumer cost

`machine_state_cost` measures owned copying/counter updates, a direct consumer
transition and the full in-memory machine path separately at 8/256/4096/65536
bytes. It reuses the sampler/allocation probe and one counter model, with whole
state and closed-form revision checks outside timing. All 12 profiles pass on
Rust 1.100 and 1.89, including the debug all-target entry; strict core benchmark
Clippy, formatting and package inclusion pass. All operation allocation probes
remain zero, with a positive allocation control.

[Raw measurements and limitations](evidence/machine-performance/state-cost.json)
retain three pinned current runs and one pinned MSRV compatibility run. The
minimal capacity-zero, effect-free counter is a separate profile: it does not
replace the broader flat/hierarchy/intent measurements. Direct computation has
no publication guarantee; its timing is not subtracted to claim exact kernel
cost. Batch-mean quantiles and optimized owned copies do not establish physical
copy counts, individual tails or release-wide performance acceptance. The durable
and timeline profiles below provide separate measurements; native end-to-end
measurements remain open.

## Durable machine performance profile

[The machine-store measurements](evidence/machine-store-performance/manifest.json)
cover one writer and 2/8/32 independent connections racing one machine head,
with exact retries, snapshot reads, race throughput, WAL size, process I/O and
whole-process RSS. All 101-round release profiles pass in three Rust 1.100 runs
and one Rust 1.89 run on four pinned cores and NVMe Btrfs, using WAL/FULL and the
existing limits. Every race checks one commit, all stale refusals, the exact
state/revision/receipt, retained payload and final event/intent counts.

Selected machine/outbox and native task/saga suites pass on both compilers.
Debug all-target entry smokes retain every writer count and invariant with three
sample rounds; the measurement tool refuses those results as performance data.
The actual debug-override refusal, 75 script tests, strict SQLite Clippy, package
inclusion and raw pilot/final runs are retained. No portable production source,
legacy benchmark fixture, timeout or validation check changed.

The shared measurement tool preserves its legacy default and supports the new
benchmark and explicit threaded affinity. Shared model/codec/async preparation
comes from the existing integration fixture. The multicore refusal tail is a
capacity finding: requests acquire a writer transaction before duplicate/stale
checks, and the same-head race rate differs sharply from winner latency. This
profile does not qualify external execution, optional snapshot cadence, other
payload widths or final release performance. Existing review findings and final
qualification remain open.

## Task read simplification review

[The task-reader evaluation](evidence/maintenance/task-read-evaluation.json)
retains a rejected single-statement experiment and its original/current pilot
measurements. Fewer queries produced more implementation code and higher observed
replay medians in that small, unbalanced pilot; it was not adopted. The retained
production change only reuses the existing nullable digest decoder, preserving
all queries, transaction boundaries and frontier checks.

The SQLite library/task suites pass on both supported compilers, including new
record-read retirement interleaving and malformed-frontier cases. The expanded
tests also pass against the original reader. Both record and blob reads now
share the real concurrent-retirement test fixture. Strict all-target SQLite
Clippy and formatting pass. These selected checks do not replace final release
qualification or resolve the existing performance findings.


## Timeline over machine performance

[The timeline-machine measurements](evidence/machine-timeline-performance/manifest.json)
cover 8/256/4096/65536-byte application state and replay horizons 1/8/32/128,
using the same computation through the existing native API and machine adapter.
All 64 profiles pass in three Rust 1.100 release runs and one Rust 1.89 run,
with 101 individual call samples after eight warmups and separate zero-allocation
probes. A closed-form oracle checks every state word after prediction and after
replaying an independently changed authority snapshot. Timeline heads and machine
provenance are checked outside timing. No second correction implementation exists.

The 73 timeline tests pass on each compiler, as do the new benchmark's debug
smokes, strict all-target Clippy, formatting and package inclusion. The benchmark
shares its model, computation, driver and existing allocation probe across paths.
Its native comparator uses the current engine's pre-existing API; the machine
path carries additional context/provenance and owned-state costs. This is a new
profile, not a replacement for frozen legacy comparisons or an acceptance of their
open performance findings. Native end-to-end execution and final qualification
remain open.

## Protected native pipeline measurement

[The native pipeline measurements](evidence/native-pipeline-performance/manifest.json)
run the same protected consumer through initial machine commit, task admission,
lost-ACK recovery, a real Linux worker, output verification, machine result commit
and saga admission. A no-op observer preserves the ordinary CLI; the private
benchmark observes twelve existing phase boundaries without copying the workflow.
Three Rust 1.100 release runs and one Rust 1.89 run each retain 101 samples after
eight warmups. Independent raw checks cover phase identity/order, interval sums,
nearest-rank quantiles and every expected result/provenance/recovery observation.

Both ordinary CLI profiles and debug benchmark smokes pass on each compiler.
A digest-pinned worker returning the wrong meaning is refused by both the CLI and
benchmark before machine result admission on each compiler. Strict consumer
all-target/all-feature Clippy and formatting pass. No library production code,
lease policy, timeout, worker sandbox or durable validation changed.

This is a sequential fresh-store recovery scenario, including console output,
fsync and cleanup. The native leg includes worker execution, IPC, polling sleeps,
durable completion and fencing; worker CPU is not isolated. The saga action is
admitted but remains undispatched. Queue saturation/backpressure performance,
other payload/effect profiles and exact release qualification remain open.

## Bounded journal backpressure

[The queue pressure measurements](evidence/machine-backpressure-performance/manifest.json)
cover capacities 1/8 and bursts 32/128 through the real public machine journal.
An independent SQLite writer lock prevents commit progress while the burst fills
the shared worker queue. Excess requests return `Capacity` on their first poll.
After release, exactly one admitted request commits and the rest refuse the
stale head. Every round checks the exact retry, retained payload/intent, snapshot,
revision and final row counts; SQLite integrity remains valid.

Three Rust 1.100 runs and one Rust 1.89 run each pass 101 rounds after eight
warmups. Debug smokes retain all dimensions and checks, and an actual debug-mode
measurement attempt is refused. Four existing worker queue/cancellation/panic
tests pass on both compilers, alongside strict benchmark Clippy, 75 script tests,
formatting, package inclusion and the runner's unchanged legacy default. The
sampler reuses the existing integration fixture and measurement tool.

First-poll refusal and deliberately gated completion have separate per-call
sample populations. Tiny refusal timings include clock/allocator/codegen effects.
This measures machine journal backpressure; native process saturation and isolated
worker CPU remain outside its scope. Final source qualification and existing
legacy performance findings remain open.

## Encoding layout followup

[The controlled encoding experiments](evidence/legacy-balanced/encoding-layout.json)
retain two sets of fifteen rotated three-variant rounds. Removing only the unused
domain-to-core dependency edge from the patched original reproduces part of the
slowdown, not its full magnitude. The serializer implementation and frozen
fixture are byte-identical across that comparison.

Two further diagnostic links of current source have identical 279510-byte `.text`
sections, placed 32 bytes apart. Their median encoding-loop rates are 358.35 and
310.24 ns/op; the normal current layout measures 360.03 ns/op in these rounds.
This reproduces a layout effect comparable to the original performance flag.
Raw code bytes, ELF layouts, symbols, whole-process counters and every timing
remain available. The exact microarchitectural mechanism is not isolated.

These are medians of process-loop averages, not individual-call latency tails.
The explicit pair controls placement; its ELF section ordering differs from the
normal link. No production linker setting, code padding, serializer change or
performance waiver was adopted. The original paired flag and final qualification
remain open.


## Read-only resolution before machine writer admission

The [scoped preflight evidence](evidence/machine-preflight-performance/manifest.json)
records a cached read-only head-revision hint and one shared complete admission
check. An exact retry or already stale request can resolve from committed data
without entering the writer queue. New writes still repeat full validation under
`BEGIN IMMEDIATE`; authorization, payload bounds, CAS, resource accounting and
post-write verification remain in force. No new public API or concurrency limit
is introduced.

A regression test holds a real SQLite write transaction open while checking stale,
conflicting-event and exact duplicate requests. The original implementation fails
the stale assertion with `Unavailable` after its five-second busy timeout; the
candidate returns the validated outcomes while the lock remains held. The full
SQLite suites pass on Rust 1.100 and MSRV 1.89, including corruption, independent
writer races, deferred commit failures and actual process-death boundaries.
Strict all-target/all-feature SQLite Clippy and formatting also pass. Both
compiler debug runs exercise all writer and bounded queue-pressure profiles; an
additional complete release writer fixture also passes on MSRV 1.89.

Six full unchanged writer fixtures run in B/C/C/B/B/C order with 101 measured
rounds per 1/2/8/32-writer profile. At 32 writers, the median of run p50 race times
moves from 229.08 to 179.31 ms (21.7% lower). This remains a same-head race with one
winner, not independent-machine throughput. One candidate run still has a
1030.55 ms p99; simultaneous callers can all observe the old head and wait for
the write lock. There is no bounded worst-case latency claim.

The first candidate showed an adverse single-writer pilot. Follow-up controls
restrict the identical fixture to one writer and alternate 15 pairs. Removing an
unnecessary event-index probe simplifies the hint; caching its prepared statement
also avoids repeated SQL compilation. In the retained cached variant, the median
per-pair p50 change is +1.06% for commits and -1.31% for exact retries. Full-fixture
single-writer run medians differ by +4.52%; storage and host variation remain.
Raw samples, all three candidate iterations and exploratory bootstrap intervals
are retained. These measurements do not prove universal equivalence or waive the
separate legacy encoding/timeline findings.

The ongoing multi-round chaos campaign began before this source change. Its
results remain development rehearsal evidence; exact final-source coverage, CI,
chaos and package qualification are still required.


## Canonical first-machine onboarding

The [onboarding checkpoint](evidence/qualification/first-machine.json) qualifies
source `36d8cb0` and retains complete commands, source hashes and dependency locks.
The first tutorial now uses the existing `MemoryMachine::handle` for immediate
preparation, validation and publication, with an explicit accepted-outcome check.
Its imports stay within `machine::*`: `machine::Digest` reexports the existing
shared content-identity type, also used by the counter, timeline and durable
examples. Existing task imports and all identity/authorization rules remain.

The independent consumer compiles the canonical guide's Rust fence directly as
`first-machine`; the README runs it and both stable/MSRV CI jobs invoke it. There
is no second tutorial model. A clean shallow clone of the actual pushed branch
runs the README command on Rust 1.100 and 1.89. A fresh `cargo new` application
using the README's Git branch dependency, exact copied guide and one generated
lockfile also prints `count = 3` on both compilers, without workspace patches.

Both compilers pass all-feature core tests, no-default/serde bare-metal core
checks, timeline and durable progressions, and the first-machine consumer.
Strict all-target/all-feature consumer Clippy, formatting and action pins pass.
An edited invalid guide fails generated-code compilation; restoring it passes.
CRLF guide copies also run on both compilers after extractor normalization. These
line-ending checks run on Linux and do not establish native Windows execution.
An initial temporary verifier expected different compiler error wording; its
failed assertion and actual successful negative refusal remain in the archive.

This improves the existing entry path without introducing an identity policy,
state owner or evaluator. Scope, event identity, digest and logical time remain
application-supplied. Full exact-candidate release qualification and the remaining
performance findings are still open.


## Portable encoding hint evaluated without retention

The [inlining diagnostic](evidence/legacy-balanced/encoding-inline.json) compares
source `a49f58a` with one isolated `#[inline]` attribute on the macro-generated
canonical-wire methods. The frozen fixture remains byte-identical to original
0.4 source. A separate diagnostic cycles four DTOs, black-boxes both the input
and complete output, checks full bytes against `serde_json::to_vec` before timing,
and verifies aggregate output size.

Across 15 rotated rounds, normal frozen loop medians are 366.02 versus 361.22
ns/op; the median paired change is -0.60%, with an exploratory interval spanning
zero. The opaque profile improves from 371.61 to 335.84 ns/op, and whole-process
instruction medians fall from about 861.08M to 813.59M. Frozen whole-process
instructions likewise decrease from about 854.80M to 805.03M. Counters include
outcome-log append; they do not isolate serialization instructions.

A diagnostic-only four-position link grid retains identical `.text` bytes within
each source/fixture variant. The hint improves the sampled opaque profile at all
four placements, but frozen elapsed time is worse at three matched placements
and better at one. This does not establish a reliable elapsed-time fix for the
retained frozen-workload finding. These are process-loop mean measurements, not
individual-call latency percentiles, allocation or stack measurements.

The hint and extra benchmark are not retained. Production serialization, capacity
hints and the frozen fixture stay unchanged; no linker setting or performance
exception is adopted. Raw source patches, code bytes, binary identities, compiler
commands, counters and every timing remain available, including the driver's
initial invalid-reference failure before any build. Original encoding/timeline
findings and exact final-source release qualification remain open.


## Deterministic preflight and bounded-history failure paths

The [failure-path checkpoint](evidence/qualification/machine-failures.json)
qualifies source `bd9ab62`. A child process pauses after its read preflight has
observed the old head and released its snapshot. A second connection commits
before the child acquires its write transaction. Duplicate, conflicting identity
and stale-head cases then assert the required result and unchanged complete
head/event/intent rows. The pause hook exists only in Unix test builds.

A full-history test fills the configured four-record bound, rejects a fifth event
without changing rows, and reconciles an exact old retry with its original
receipt. Scoped SQLite coverage newly observes the transactional duplicate-return
region and history-capacity return. Removing transactional admission makes the
new race test fail at duplicate reconciliation (remaining consistency checks
return `Corrupt`); removing the history ceiling makes the new capacity test fail.
These mutation controls do not establish that the first mutant commits bad data.

The same test-only pause also extends commit SIGKILL recovery to eight boundaries.
Four repeated marker waits share one helper, preserving deadlines, process
cleanup and crash assertions. Full SQLite tests pass on Rust 1.100 and MSRV 1.89;
strict all-target/all-feature SQLite Clippy and workspace formatting pass.

Fresh workspace all-feature coverage records 14,874/15,807 lines, **94.0976%**,
above the unchanged 94.0759% ratchet. The margin is narrow; this is not evidence
of complete failure-path coverage. The archive retains full region reports,
commands, source snapshots, mutation patches and logs, including an initial
mutation-driver output-directory collision before any build. All 38 archived
members were verified against their recorded hashes. This remains a development
checkpoint; final-source release qualification and performance disposition remain
open.


## Current-source legacy timeline comparison

The [current timeline checkpoint](evidence/legacy-balanced/timeline-current.json)
compares original 0.4 source `42ec187` with `94a7396`: 31 alternating measured
pairs, 640 unchanged profiles and 1,000 samples per profile per process. The
existing driver performs one warmup pair. All observations and p50/p95/p99
measurements remain in the archive, alongside whole-process counters, exact
source fixtures, original binaries, code sections and symbols. Original binary
hashes match the measured artifacts; copies transformed while extracting code
sections are identified separately. No measured executable was modified.

Two p50 profiles and one p95 profile cross the existing review thresholds:

| Profile | Metric | Original | Current | Change |
| --- | --- | --- | --- | --- |
| `history_capture`, 1 word, 1 step | p50 | 251 ns | 270 ns | +7.57% |
| `ring_replay`, 128 words, 1 step | p50 | 50 ns | 60 ns | +20.00% |
| `history_append`, 128 words, 32 steps | p95 | 291 ns | 321 ns | +10.31% |

`ring_replay` is the handwritten benchmark comparator: its timed closure calls
`Ring::replay` and the fixture's integer computation, without invoking Penelope's
replay API. Its flag is retained as a control observation, not attributed to a
library replay implementation. This distinction does not normalize any result
or waive a finding.

The first profile repeats a historical flag. The latter two exploratory paired
intervals span zero; this is not an equivalence test or permission to discard
them. The earlier nine p50/two p95 findings remain retained. Different current
flags do not establish their cause, statistical equivalence or acceptance. The
next focused diagnostic is the recurring history-capture case.

Measurements use CPU 4; the concurrent chaos root was observed on CPUs 8–15.
Shared host applications, frequency, caches and memory bandwidth remain
uncontrolled. Binary copying and inspection occurred during the campaign; no
observations were removed. All 101 archived members passed hash verification.
The archive also records all seven successful GitHub CI jobs for the exact
measured revision ([run 43786989284](https://github.com/STEXS-Technologies/penelope/actions/runs/43786989284)).
This is a development checkpoint, not final candidate or packaged-release
qualification. Performance disposition and the full chaos campaign remain open.


## Focused history-capture placement diagnostic

The [capture placement diagnostic](evidence/legacy-balanced/timeline-capture-layout.json)
uses the original workload helper with a temporary reporter selecting only
`history_capture`, one word and one step. It preserves that helper's state model,
setup, capture closure and latency reporter. Filtering changes code generation;
these timings therefore do not replace the complete frozen-workload campaign.

Eight binaries cover original/current source and four `.text` placements, with
15 rotated rounds and 10,000 samples per process. Within each source variant,
all four placements retain identical `.text` bytes. Process-median p50 results:

| `.text` placement | Original source | Current source |
| --- | --- | --- |
| `0x20000` | 271 ns | 301 ns |
| `0x20020` | 311 ns | 301 ns |
| `0x20040` | 280 ns | 291 ns |
| `0x20060` | 310 ns | 301 ns |

Placement changes the elapsed result and reverses which source is faster. This
establishes layout sensitivity in the focused fixture, not equivalence or an
accepted disposition of the frozen 251→270 ns finding. Whole-process counters
include setup and reporting; they do not isolate capture instructions. Frequency,
caches and memory bandwidth remain uncontrolled.

Inspection of the original retained full-workload binaries finds 284 instructions
and size `0x60d` in each one-word `check_anchor_admission` specialization. Assembly
matches after normalizing addresses and RIP-relative relocation displacements.
This comparison is limited to that function and is not whole-program equivalence.
Raw assembly, normalization output, all measurements, binaries, hashes, temporary
source and build arguments remain in the archive; all 317 members passed hash
verification. No focused benchmark, linker flag or production change is retained.
The exact microarchitectural cause and performance acceptance remain open.


## Native worker capacity and slot reuse

The [native slot-reuse checkpoint](evidence/qualification/native-slot-reuse.json)
qualifies source `8315bb4`. The existing one-worker admission test now durably
authorizes a second task through the real supervisor/journal. Capacity refusals
preserve the first sleeping worker's registry identity. Fencing it permits the
exact second request to start; its reserved retry succeeds, polling remains
pending, and fencing prevents a later restart. The supervisor whose dispatch
failed still requires reconstruction; runtime slot reuse does not bypass that
control rule. No production behavior changes.

All 67 runtime integration tests pass on Rust 1.100 and MSRV 1.89. Strict runtime
test Clippy, formatting and diff checks pass. Initial Clippy/partial-rename
failures remain in the archive; all 34 archived members passed hash verification.
This is correctness evidence at `max_live=1`, not worker throughput, CPU isolation
or a general saturation-performance qualification. Those scopes and final
release qualification remain distinct and open.


## Tests from independently packaged archives

The [independent package checkpoint](evidence/packaging/independent-tests.json)
qualifies source `d3ec00b`. All ten development archives pass Cargo library
verification and literal-include validation. An extracted workspace patches each
Penelope dependency to its matching archive; Cargo metadata confirms all ten
resolve there. Full workspace all-target/all-feature tests pass on Rust 1.100
and MSRV 1.89 with the same normalized lockfile. This includes real-worker,
SQLite recovery/preflight and native slot-reuse checks plus benchmark smoke
runs; these debug runs are not release performance measurements or doctests.

The initial archive run exposed two executor architecture checks that assumed
checkout-only source/fuzz files. All six architecture tests move byte-identically
to the existing compatibility CI harness, with explicit stable/MSRV/release
execution, formatting and strict Clippy. Both compilers pass them. The executor
archive no longer carries those repository tests or a direct `syn` dev dependency.
No architecture condition is skipped or relaxed.

Constructing the reduced archive workspace initially refuses the root lock under
`--locked`. Offline normalization removes only three fuzz-only dependencies;
no external version or checksum changes. Subsequent tests use `--locked`. The
archive retains both original and corrected crate sets, manifests, locks, source,
commands and failure/success logs; all 74 members passed hash verification.
At that checkpoint, crate versions remained 0.4.0. This is package-isolation evidence,
not final versioned 0.5 candidate qualification or publication.


## Versioned 0.5 development checkpoint

The [candidate checkpoint](evidence/packaging/candidate-checkpoint.json) records
source `9a5a1db`. All ten libraries and their active internal requirements use
0.5.0. The six active lockfiles retain every external package identity and
checksum; frozen baseline locks and corpora are unchanged. Locked metadata
resolves the library crates inside the isolated candidate checkout.

Legacy saga parity, compatibility tests (including all six repository architecture
checks) and actual SQLite upgrade checks pass on Rust 1.100 and MSRV 1.89. The
seven selected signed-source conformance tests pass against pinned StateChronicle
`ae3d94bc`. Library all-target/all-feature tests and debug benchmark smoke runs
pass on both compilers. Cargo verifies all ten archives and their literal Rust
includes; every archive records source `9a5a1db`. All 242 local Markdown link
paths resolve (this does not verify remote links, anchors or rendering).

The archive retains commands, source manifests/locks, crate archives and raw logs.
It also retains the initial refusal of an unpinned upstream checkout and two
commands deliberately stopped after an omitted fuzz exclusion launched unbounded
fuzz targets. The corrected documented library commands passed; the separate
bounded chaos rehearsal was untouched. This checkpoint is neither final release
qualification nor publication. Extracted-archive tests, final-source CI, coverage,
chaos and performance decisions remain separate gates.


The same candidate code also passes all six canonical consumer binaries on Rust
1.100 and 1.89, including the actual first-machine tutorial and durable
machine/task/saga progression. Both compilers check core (bare and serde) and
timeline (bare and alloc) on `riscv32imafc-unknown-none-elf`. Stable doctests,
strict all-target/all-feature Clippy (workspace and compatibility harness), and
warning-denying API documentation pass. The checkpoint archive includes all
commands and logs. These checks ran at `feabc53`, whose source/manifests/locks,
CI and scripts match `9a5a1db`; only documentation and evidence differ.
