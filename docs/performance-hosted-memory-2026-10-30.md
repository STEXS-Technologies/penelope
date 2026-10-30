# Hosted supervisor comparison — 2026-10-30

The shared-controller hosted wrapper at `683b90e` regresses sampled stack use
against the pinned 0.2.0 baseline `edcc5b423e4919f51e4aa2cb820d9de68a684145`.
This evidence keeps hosted memory/performance qualification open. It is not a
release acceptance report.

The [comparison method](hosted-task-memory-method.md) uses identical fixed-port
fixture source per capacity, Rust 1.99.0, CPU 0, separate locked builds, one native
warmup per revision/path and alternating paired revision order. Three pairs ran
at each capacity after the database diagnostic process finished. No other agent
build or stress campaign ran during these measurements. OS caches, CPU frequency
and unrelated host activity remain uncontrolled.

All measured lifecycle intervals asserted zero allocations, zeroed allocations,
reallocations, deallocations and requested bytes after positive calibration.
Both revisions returned the expected output, start/fence counts and checkpoint
ACK count through pure recovery and effect reconciliation. This characterizes
these fixed in-memory ports, not SQLite or the Linux runtime.

Maximum retained Massif stack samples, identical across all three pairs:

| Capacity / whole-process case | Baseline bytes | Candidate bytes |
| --- | ---: | ---: |
| 4,096 / startup baseline | 68,640 | 76,864 |
| 4,096 / 64 KiB positive control | 134,008 | 142,232 |
| 4,096 / pure recovery | 85,056 | 106,896 |
| 4,096 / effect reconciliation recovery | 85,056 | 106,896 |
| 65,536 / startup baseline | 68,640 | 251,616 |
| 65,536 / 64 KiB positive control | 134,008 | 316,984 |
| 65,536 / pure recovery | 202,304 | 281,648 |
| 65,536 / effect reconciliation recovery | 202,304 | 281,648 |

These are whole-process samples including fixture ownership/moves, compiler
frame placement, startup and reporting. The capacity-dependent startup maximum
in the candidate illustrates why a baseline subtraction cannot isolate library
stack cost. These are neither static worst-case bounds nor proof of a particular
wrapper frame size. The sampled lifecycle increase is 21,840 bytes at 4 KiB
capacity and 79,344 bytes at 64 KiB capacity.

Both revisions report a 13,656-byte supervisor with the fixed journal/runtime
and a 416-byte supervisor with zero-sized ports. Object layout therefore does
not explain the stack regression. Source inspection shows the candidate wrapper
constructs three capacity-sized arrays in both advance and poll; the baseline
constructs two for dispatch and one for polling. This is a candidate cause,
not an isolated causal measurement of those arrays.

Native process measurements run 10,000 complete lifecycles per invocation.
GNU time's 0.01-second wall resolution is coarse for these durations:

| Capacity / path | Baseline wall seconds, each run | Candidate wall seconds, each run | Baseline RSS range, KiB | Candidate RSS range, KiB |
| --- | ---: | ---: | ---: | ---: |
| 4,096 / pure | 0.02 | 0.03 | 2,132–2,200 | 1,936–2,196 |
| 4,096 / effect | 0.03 | 0.04 | 1,988–2,236 | 1,984–2,236 |
| 65,536 / pure | 0.05 | 0.08 | 2,392–2,492 | 2,208–2,424 |
| 65,536 / effect | 0.06 | 0.12 | 2,176–2,492 | 2,116–2,408 |

These timings motivate a longer follow-up; they are not precise latency ratios
or throughput promises. RSS covers the entire process and is not incremental
supervisor memory. Native timings and instrumented stack runs are separate.

[Raw records](performance-data/2026-10-30-hosted-memory/) retain both capacities,
48 Massif profiles including calibration cases, fixture/build output, raw native
time/RSS records, allocation assertion output, lock/binary/runner hashes and
machine/tool metadata. The runner also retains warmup output. No failing or
slower result was removed. Reproduction uses the commands in the method report,
with `--after 683b90e`, both capacities, three runs and 10,000 repetitions.
