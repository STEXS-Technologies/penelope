# Initial stabilization and facade verification

This is an incremental verification ledger, not 0.5 release qualification.
Earlier sections describe their original checkpoint; later sections record the
machine kernel, hierarchy, shared admission and prepared adapters. The durable
machine protocol remains pending.
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
and tests it. The development toolchain remains independently pinned to 1.99.

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
