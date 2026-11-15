# Initial stabilization and facade verification

This is an incremental checkpoint, not 0.5 release qualification. The machine
kernel, hierarchy, adapters and durable machine protocol remain unimplemented.
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

## Candidate changes

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
