# Initial stabilization and facade verification

This is an incremental verification ledger, not 0.5 release qualification.
Earlier sections describe their original checkpoint; later sections record the
machine kernel, hierarchy, shared admission, prepared adapters and initial scoped
journal/SQLite snapshots. Outbox delivery and durable bridges remain pending.
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
