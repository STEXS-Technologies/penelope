# Final development performance review — 2026-10-30

The [graph optimization follow-up](performance-graph-optimization-2026-10-30.md)
supersedes this report's graph slowdown disposition: the optimized workload
measures 15.24% lower median time than the original baseline. This report retains
the historical measurements and other evidence at their pinned source revisions.

Baseline `edcc5b423e4919f51e4aa2cb820d9de68a684145` was compared with production
candidate `50fec4ee9dd58733199d48ba36f7075c056e06ea`. Later changes through
`0f4080b` add runtime tests and documentation. These measurements preceded the
0.3.0 metadata update; they do not describe every future compiler/profile/host.

## Method and evidence

[Raw compressed records and SHA-256 manifest](performance-data/2026-10-30-final-qualified/manifest.json)
retain all 240 comparison records, 40 self-control records, metadata, perf
summaries and three worker runs. Rust 1.99.0 on Ryzen 9 7950X; CPU 0 for serial
work and CPUs 0–3 for parallel work. Twenty paired rounds followed one warmup,
with alternating revision order. Both archived snapshots used their locked
versions, identical benchmark sources and direct rustc invocation. The first
attempt failed before measurement when the host sccache daemon timed out; the
[gate evidence](verification-data/2026-10-30-final-development-gates/metadata.json)
retains that failure and retry configuration. The chaos campaign finished before
measurement, and final correctness checks started after all measurements.
Frequency and OS caches were uncontrolled. No cold-cache or universal latency
claim follows. Perf includes the time wrapper, benchmark and inherited threads.

## Existing decision benchmarks

Marginal medians, nanoseconds per operation; parallel operation units use the
benchmark's aggregate worker accounting. RSS is whole-process peak, including
runtime overhead, and does not measure incremental library allocation.

| Workload | Baseline | Candidate | Median change | Median peak RSS KiB, before → after |
| --- | ---: | ---: | ---: | ---: |
| Linear start/result/complete | 268.005 | 269.530 | +0.57% | 2108 → 2128 |
| Graph start/result/result | 963.960 | 993.862 | +3.10% | 2160 → 2164 |
| Linear replay | 412.421 | 410.713 | -0.41% | 2106 → 2128 |
| Parallel linear | 75.006 | 72.249 | -3.68% | 2170 → 2180 |
| Parallel graph | 287.805 | 286.115 | -0.59% | 2176 → 2186 |
| Canonical DTO encoding | 332.376 | 321.401 | -3.30% | 4218 → 4218 |
| Bounded outcome append | 506.120 | 509.348 | +0.64% | 4218 → 4218 |

Parallel linear's paired geometric-mean time difference is +2.43%, with an
exploratory paired-bootstrap interval of -2.74% to +9.69%. Its marginal median
has the opposite sign because of outliers. Task-clock is +0.022% paired and
instructions -0.0018%. Baseline-versus-baseline control gives -2.42% paired wall
time, interval -7.03% to +1.46%, with +0.388% task-clock. These data do not prove
noninferiority or exclude smaller regressions. They do not reproduce the
initial +12.9% marginal median concern. The engine and benchmark source remain
unchanged; source equality alone is not performance proof.

Graph-saga has a repeatable measured regression in this campaign: paired time
+2.98%, exploratory interval +2.18% to +3.74%; task-clock +2.96%, cycles +3.02%,
instructions +0.114%. The graph engine and benchmark sources are unchanged.
The cause is unresolved; generated-code layout and host effects are possible,
not established explanations.

**Qualification decision:** accept the observed graph median cost of 29.90 ns
per three-transition benchmark operation, and the recorded +0.57% linear and
+0.64% append medians, as explicit performance exceptions for the 0.3.0 reference
profile. The release adds typed, independently verified task integration while
preserving the existing decision path and keeping optional storage/process I/O
outside it. The measured absolute graph cost remains below one microsecond per
operation on this profile. This is a release tradeoff, not evidence that the
regression is necessary or harmless for every consumer. Retain the raw data and
report the regression to consumers with tighter latency budgets. No throughput
improvement, timing parity, statistical noninferiority or universal bound is
claimed. This explicit decision supersedes the open performance disposition in
the earlier [parallel follow-up](performance-parallel-followup-2026-10-30.md).

## Sealed-executable worker dispatch

Three serialized runs each contain 64 measured tasks per fresh/reused profile
and one excluded warmup per profile: 384 measured and six warmup worker launches.
CPU 0, static C worker, local Btrfs, SQLite WAL/FULL, persistent registry fsync,
no durability bypass. Fresh means newly constructed store/runtime; OS caches are
uncontrolled. ACK means the host checkpoint ACK write completed, not independent
worker/application receipt. Each task verifies output and confirmed fencing.

Ranges of the three run-specific medians, milliseconds; raw records retain each
run's p95/p99 and all samples rather than treating three runs as population tails.

| Profile | Start return | Dispatch → host checkpoint ACK | Dispatch → completion and fence |
| --- | ---: | ---: | ---: |
| Fresh store/runtime | 3.298–3.333 | 4.018–4.049 | 5.825–5.907 |
| Reused store/runtime | 2.875–2.986 | 3.637–3.744 | 5.472–5.779 |

The new adapter has no v0.2 runtime counterpart. These data establish its scoped
baseline, not an improvement comparison or exactly-once external effects.

The other qualification evidence remains in the [SQLite cache report](performance-sqlite-cache-2026-10-30.md),
[durable store baseline](performance-sqlite-2026-10-30.md),
[hosted allocation/stack comparison](performance-hosted-borrowed-restore-2026-10-30.md),
and [RISC-V closed call-chain bound](portable-task-stack-bound-2026-10-30.md).
Those reports retain their exact profile, source revision and scope limits.

## Versioned 0.3.0 candidate confirmation

After the metadata update, candidate
`16e3e1e3e3c7038aea4f3708cb9cc519fedaf664` passed the full local check sequence.
A second serialized twenty-pair six-binary comparison then ran with the same
method, followed by three worker runs and the static RISC-V analysis. This
refresh accounts for compiler crate identity/layout changes after versioning;
both campaigns remain retained. [Versioned records and hashes](performance-data/2026-10-30-versioned-qualified/manifest.json)
are authoritative for the versioned candidate.

| Workload | Baseline ns/op | Candidate ns/op | Median change | Median peak RSS KiB, before → after |
| --- | ---: | ---: | ---: | ---: |
| Linear start/result/complete | 270.850 | 272.442 | +0.59% | 2118 → 2078 |
| Graph start/result/result | 968.562 | 997.088 | +2.95% | 2132 → 2146 |
| Linear replay | 412.661 | 406.940 | -1.39% | 2140 → 2128 |
| Parallel linear | 75.314 | 72.303 | -4.00% | 2154 → 2190 |
| Parallel graph | 284.200 | 286.645 | +0.86% | 2178 → 2212 |
| Canonical DTO encoding | 333.678 | 321.298 | -3.71% | 4432 → 4236 |
| Bounded outcome append | 509.274 | 503.169 | -1.20% | 4432 → 4236 |

Graph's paired time difference is +3.44%, exploratory interval +2.43% to +4.55%;
task-clock +3.41%, cycles +3.45%, instructions +0.112%. Its 28.53 ns marginal
median cost confirms the preceding explicit exception; the cause remains
unresolved. The +0.59% linear and +0.86% parallel-graph medians are also accepted
under the same reference-profile tradeoff. Parallel linear's paired time is
-2.92%, interval -10.12% to +3.06%; task-clock +0.112%, instructions -0.0015%.
No interval establishes noninferiority, and the differing signs/variation are
retained. No further run was selected to replace an inconvenient result.

For the versioned worker runs, ranges of run-specific medians in milliseconds:

| Profile | Start return | Dispatch → host checkpoint ACK | Dispatch → completion and fence |
| --- | ---: | ---: | ---: |
| Fresh store/runtime | 3.380–3.448 | 4.093–4.167 | 5.928–6.084 |
| Reused store/runtime | 3.010–3.106 | 3.759–3.986 | 5.708–6.450 |

The versioned RISC-V analysis again closes 42 reachable functions at a
**115,056-byte** conservative stack bound, with **65,552 bytes** for the positive
control. The retained generated profile, frame inventory, disassembly and ELF
section accounting preserve its exact toolchain/source scope. This is the same
fixed-port, aligned-entry, no-interrupt profile described in the original stack
report, not a bound for arbitrary adapters or application call chains.
