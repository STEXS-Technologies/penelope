# Graph performance follow-up — 2026-10-30

The former 2.95–3.10% graph-saga slowdown is superseded by the optimized source
at `edf11943113b7a65b80b9fae5c3ac43ebcef2da9`. In a fixed forty-pair comparison
against the original 0.2.0 baseline `edcc5b4`, serial graph-saga's marginal median
is **15.24% faster**, with **17.73% fewer executed instructions**. This exceeds
the requested goal of reducing that benchmark's overhead to 1% or less.
It does not establish a universal latency guarantee or a formal noninferiority
test across deployments.

## Changes and invariant preservation

Graph result handling now clones only the fields retained by the successor.
It avoids cloning current-step/active-action identities that are immediately
replaced and reserves the history slot for a successor action before copying
issued-action identities. All required scope/history ownership is retained.

Graph definition validation sorts a vector of borrowed step identities and uses
binary search for the same reference checks. A 128-entry reachability bitmap
replaces its temporary visited tree, with nodes marked when queued. Duplicate
transition detection, graph/edge bounds, reference validation, reachability,
error precedence and runtime result validation remain enforced. No validation
cache, skipped validation or new wire/API format is involved.

`graph_validation_parity.rs` retains the independent original tree-based
validator as an oracle. Two property tests each ran 10,000 requested cases on
stable and MSRV: arbitrary malformed definitions compare exact errors; valid
chains/cycles compare acceptance with reordered steps. Explicit cases cover all
ten error categories, and a maximum-size branching cyclic graph verifies the
indexed traversal. Existing graph/replay/task integration suites also run.

## Method and all retained campaigns

[Metadata, all measurements and SHA-256 manifest](performance-data/2026-10-30-graph-optimization/manifest.json)
retain each stage, including unfavorable and noisy results. The existing six
benchmark binaries, source/datasets and locked dependencies remain unchanged.
Rust 1.99.0, Ryzen 9 7950X, CPU 0 for serial work and CPUs 0–3 for parallel work;
one excluded warmup and alternating revision order, Linux perf and GNU time.
No local correctness/build campaign ran during a measured loop. Unrelated
compilation on the same host was observed; frequency, OS caches and competing
workloads were uncontrolled. Every revision has its own archived build, and
hardware counters include the time wrapper, benchmark and child threads.

| Stage | Candidate | Paired rounds | Serial graph median difference | Disposition |
| --- | --- | ---: | ---: | --- |
| Remove discarded clones | `7feecf3` | 20 | +1.70% | Improvement, but short of requested target |
| Reserve action history | `c6ca7d5` | 20 | -11.92% | Host-contaminated timing: unchanged linear control moved -26.49%; retained, not used as the confirmation claim |
| Indexed graph validation | `edf1194` | 40 | -15.24% | Confirmation campaign; instructions also decreased substantially |

The final forty-pair count was selected before that campaign began. The
algorithm changed between stages; no repeated unchanged run was selected to
replace an inconvenient result. Both the noise and unchanged controls remain
visible in the raw records.

## Final six-binary results

Marginal medians in nanoseconds per benchmark operation. Parallel units retain
the existing benchmark's aggregate-worker accounting. These are pure engine/
contract workloads, not SQLite/process or external-effect timing.

| Workload | Baseline | Optimized | Median change |
| --- | ---: | ---: | ---: |
| Linear start/result/complete | 281.913 | 278.811 | -1.10% |
| Graph start/result/result | 1005.376 | 852.197 | -15.24% |
| Linear replay | 442.060 | 438.863 | -0.72% |
| Parallel linear | 102.759 | 104.584 | +1.78% |
| Parallel graph | 366.445 | 299.595 | -18.24% |
| Canonical DTO encoding | 346.593 | 335.432 | -3.22% |
| Bounded outcome append | 529.877 | 543.188 | +2.51% |

For serial graph, paired geometric-mean time is -15.35%, with an exploratory
95% paired-bootstrap interval of -20.20% to -10.07%. Paired task-clock is -15.15%,
cycles -15.13%, instructions -17.73%. Parallel graph's paired time is -17.73%,
interval -24.08% to -11.62%; instructions -17.34%. Ten thousand bootstrap
resamples use seed 3030. These descriptive intervals do not establish a
universal bound; the substantial instruction reduction supports reduced work
independently of elapsed-time variation.

**Disposition:** the original graph slowdown exception is removed for this
reference workload. This does not mean every benchmark is within 1%: parallel
linear and outcome append have positive medians in this host-contended campaign,
and their unchanged code/near-identical instruction counts do not rule out
smaller timing or generated-code-layout regressions. Their measured reference-
profile tradeoff remains disclosed; no all-workload timing-parity claim follows.
The original [performance review](performance-final-2026-10-30.md) remains the
historical durable-store/worker/portable-memory baseline at its pinned revisions.

## Validation scope

The changed graph/linear fuzz target receives three additional 100,000-input
runs with seeds 1, 2 and 3. This supplements the earlier three-iteration full
campaign; it does not claim all 48 original target runs used the optimized code.
Full stable/MSRV workspace checks, docs/Clippy, layer checks, security checks,
coverage, package verification and optional upstream conformance run again.
The core, SQLite and Linux runtime production sources are unchanged by this
optimization; their original scope limitations continue to apply.

CI also exposed an existing test-only status-file race: overwriting a marker
can expose a truncated/partial state to the parent. Atomic write/rename publication
fixes that harness without weakening the exactly-one-CAS-winner assertion.
The actual two-process arbitration/reopen test passed 100 repetitions on each
of stable and MSRV. This test-only change at `495451f` does not change the
measured graph implementation. The failed CI log is retained.
