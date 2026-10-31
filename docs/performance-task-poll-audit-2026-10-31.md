# Pending-task polling audit — 2026-10-31

No production optimization was retained. Four attempts to avoid repeated local
admission checks either slowed polling or slowed the public observation control.
The branch now includes a reproducible benchmark for future investigation.

## Scope and fixtures

Baseline source is 0645672bfa98fae791247553441bd6be9374d3cf, including the
new `task_poll` benchmark. All four candidate patches apply to that baseline.
The final controller is byte-identical to the baseline controller.

The benchmark measures portable `TaskControl::poll_with_buffer`, public
`TaskControl::observe` as a control, and hosted `TaskSupervisor::poll`, each
with capacities of 64, 4096 and 65536 bytes. A runtime returns a scoped pending
observation through a non-inlined, black-boxed method. Setup, submission and
initial dispatch occur outside the measured loop. Each operation must return
false. Portable and observation loops run two million operations; hosted loops
run two million except the 65536-byte case, which runs twenty thousand.

This isolates a cheap pending response. It includes no real protected process,
SQLite, disk work or application computation, and says nothing about checkpoint
or completion throughput. It cannot establish application slowdown or replace
the earlier end-to-end overhead measurements.

## Exploratory results

Each candidate received one unrecorded warmup per binary, followed by eight
alternating baseline/candidate process pairs pinned to CPU 0. No project build,
test or graph update ran concurrently with timing. Other host workloads,
frequency and caches were uncontrolled. The table reports medians of paired
percentage changes calculated from full printed elapsed durations and operation
counts, not the benchmark's rounded integer ns/op. Positive means slower.

| Candidate | Portable 64 | Portable 4096 | Portable 65536 | Observe 64 | Hosted 64 | Hosted 4096 | Hosted 65536 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Shared admission helper | +14.20% | +15.40% | +13.92% | +45.65% | +17.53% | +6.94% | +0.60% |
| Helpers explicitly inlined | −18.48% | −17.53% | −18.17% | +30.81% | −16.39% | −9.50% | −0.95% |
| Pending-only return, equality | +23.63% | +12.52% | +11.21% | −3.15% | +19.31% | +5.34% | +0.38% |
| Pending-only return, pattern match | +11.25% | +7.85% | +9.29% | −0.68% | +9.68% | +3.19% | −0.89% |

All nine profiles, including the other two observation controls and individual
paired ratios, are in the retained summary. The inlined candidate's observation
regression also appears at the other capacities (+31.46%, +31.53%). Code layout
and inlining may explain these changes, but this audit did not establish a
machine-code-level cause. Baseline absolute times changed between campaigns;
comparisons are within each campaign. These short pilots support rejecting the
candidates, not precise general performance claims or statistical guarantees.

## Validation and reproduction

The retained benchmark passed optimized execution on Rust 1.99.0 and Rust 1.85.0.
Strict benchmark Clippy with all features, workspace formatting and layer checks
passed, and the AST graph was updated. Production code was restored, so this
slice does not change the previously qualified portable memory or stack profile.
The preceding SQLite fix passed all CI jobs in
[run 41151038754](https://github.com/STEXS-Technologies/penelope/actions/runs/41151038754).

Run the retained benchmark with:

```sh
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo bench -p penelope-executor --bench task_poll
```

For a fresh comparison, build that baseline first and copy its benchmark binary
out of Cargo's target directory. Apply one retained patch, rebuild and copy the
candidate binary separately. Warm both with `taskset -c 0`, then run eight pairs
in baseline/candidate order for even pairs and candidate/baseline order for odd
pairs. Preserve stdout, divide each duration by its operation count and compare
paired ratios. Do not build or test concurrently with the measured runs.

[Retained evidence](verification-data/2026-10-31-task-poll-audit/metadata.json)
includes binary hashes, toolchain versions, candidate patches, 64 measured
process stdout records, validation logs and artifact hashes. Warmup stdout was
not retained. Binary executables are not archived. Recalculate all summaries:

```sh
python3 docs/verification-data/2026-10-31-task-poll-audit/summarize.py
```
