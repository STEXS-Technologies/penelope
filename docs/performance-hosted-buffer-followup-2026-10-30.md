# Hosted buffer follow-up — 2026-10-30

Candidate `24c02fadb9f0080bfe5e0eb1a797ad0d19726fb1` reduces the hosted wrapper to two dispatch buffers and one poll buffer. The pinned baseline remains `edcc5b423e4919f51e4aa2cb820d9de68a684145`. Qualification remains open: the remaining sampled stack and native timing regressions require investigation.

The [same comparison method](hosted-task-memory-method.md) and unchanged fixture were used, with three alternating pairs per capacity, CPU 0, Rust 1.99.0 and **1,000,000 complete lifecycles per native invocation**. Native warmup precedes measurement. No other agent build or stress campaign ran concurrently. OS caches, CPU frequency and unrelated host activity remain uncontrolled.

| Capacity | Path | Baseline wall seconds (runs) | Candidate wall seconds (runs) | Median change |
| ---: | --- | --- | --- | ---: |
| 4,096 | pure_checkpoint_recovery | 2.44, 2.43, 2.45 | 2.80, 2.98, 3.08 | +22.1% |
| 4,096 | effect_reconciliation_recovery | 3.52, 3.45, 3.37 | 4.14, 4.08, 4.50 | +20.0% |
| 65,536 | pure_checkpoint_recovery | 5.13, 5.04, 4.99 | 5.93, 5.86, 5.92 | +17.5% |
| 65,536 | effect_reconciliation_recovery | 6.22, 6.24, 6.32 | 8.59, 8.55, 8.89 | +37.7% |

These are process durations, not per-operation latency measurements or a statistical confidence interval. Native runs are separate from instrumented stack runs.

Maximum retained Massif stack samples were identical across each set of three runs:

| Capacity | Whole-process case | Baseline bytes | Candidate bytes |
| ---: | --- | ---: | ---: |
| 4,096 | stack_baseline | 68,640 | 72,752 |
| 4,096 | stack_positive_control | 134,008 | 138,120 |
| 4,096 | pure_checkpoint_recovery | 85,056 | 102,784 |
| 4,096 | effect_reconciliation_recovery | 85,056 | 102,784 |
| 65,536 | stack_baseline | 68,640 | 186,096 |
| 65,536 | stack_positive_control | 134,008 | 251,464 |
| 65,536 | pure_checkpoint_recovery | 202,304 | 216,128 |
| 65,536 | effect_reconciliation_recovery | 202,304 | 216,128 |

Compared with the [previous candidate](performance-hosted-memory-2026-10-30.md), sampled lifecycle stack falls from 106,896 to 102,784 bytes at 4 KiB capacity, and from 281,648 to 216,128 bytes at 64 KiB. The remaining difference from baseline is 17,728 and 13,824 bytes respectively. Whole-process samples include fixed journal ownership/moves and compiler frame placement; startup baselines also change with capacity. These are not static worst-case bounds, and subtracting startup does not isolate supervisor stack cost.

All native lifecycle intervals assert zero allocation, reallocation, deallocation and requested bytes after positive allocator calibration. Both revisions retain the same supervisor layouts (13,656 bytes with fixture ports, 416 with zero-sized ports), expected output, checkpoint acknowledgements and start/fence counts. These fixed in-memory ports do not characterize SQLite or Linux process adapter memory.

[Raw evidence](performance-data/2026-10-30-hosted-buffer-followup/) retains measurements, metadata, fixture source, build logs, native stdout/time/RSS and all 48 Massif profiles. The archive manifest records SHA-256 for source and archive bytes; stderr files are gzip-compressed byte-exact to retain profiler whitespace without repository whitespace violations.

Next investigation: isolate remaining ownership/restore and call-boundary overhead using the same fixture. Neither this report nor the buffer change constitutes an accepted performance exception.
