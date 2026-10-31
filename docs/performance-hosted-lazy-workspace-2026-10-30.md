# Hosted lazy-workspace comparison — 2026-10-30

Candidate `b5cd836` initializes dispatch storage only for a start/restart. Baseline is `edcc5b423e4919f51e4aa2cb820d9de68a684145`. Qualification remains open: timings still regress and sampled stack improves at 4 KiB but worsens at 64 KiB relative to the previous buffer candidate.

The [unchanged comparison fixture and method](hosted-task-memory-method.md) use Rust 1.99.0, CPU 0, identical capacity-specific source, three alternating pairs and one million complete native lifecycles per invocation. No concurrent agent build/stress work ran. OS caches, frequency and unrelated host activity are uncontrolled. The first baseline 4 KiB pair was slower than the later pairs; this is not a confidence interval or an isolated causal timing estimate.

| Capacity | Path | Baseline wall seconds | Candidate wall seconds | Median change |
| ---: | --- | --- | --- | ---: |
| 4,096 | pure_checkpoint_recovery | 3.25, 2.47, 2.42 | 2.73, 2.67, 2.76 | +10.5% |
| 4,096 | effect_reconciliation_recovery | 4.57, 3.46, 3.49 | 3.88, 3.77, 3.92 | +11.2% |
| 65,536 | pure_checkpoint_recovery | 5.12, 5.03, 5.27 | 5.46, 5.44, 5.35 | +6.2% |
| 65,536 | effect_reconciliation_recovery | 6.37, 6.40, 6.51 | 7.14, 6.84, 6.79 | +6.9% |

Maximum retained Massif stack samples were identical across all three pairs:

| Capacity | Whole-process case | Baseline bytes | Candidate bytes |
| ---: | --- | ---: | ---: |
| 4,096 | stack_baseline | 68,640 | 68,672 |
| 4,096 | stack_positive_control | 134,008 | 134,040 |
| 4,096 | pure_checkpoint_recovery | 85,056 | 98,704 |
| 4,096 | effect_reconciliation_recovery | 85,056 | 98,704 |
| 65,536 | stack_baseline | 68,640 | 120,544 |
| 65,536 | stack_positive_control | 134,008 | 185,912 |
| 65,536 | pure_checkpoint_recovery | 202,304 | 254,096 |
| 65,536 | effect_reconciliation_recovery | 202,304 | 254,096 |

Against [the previous candidate](performance-hosted-buffer-followup-2026-10-30.md), lifecycle samples decrease from 102,784 to 98,704 bytes at 4 KiB, but increase from 216,128 to 254,096 at 64 KiB. Inspection of the generated 64 KiB binary shows the fixed-buffer provider inlined into `advance_inner`; the controller restore also has a larger frame than the baseline. These observations guide separate call-frame and borrowed-restoration experiments; they do not establish the cause of the entire process peak.

All native intervals assert zero measured allocations, reallocations, deallocations and requested bytes after positive calibration, along with expected output, ACKs and start/fence counts. Supervisor layouts remain 13,656 bytes with fixture ports and 416 with zero-sized ports. Massif samples include fixture moves, startup and compiler placement; they are not static worst-case bounds. These fixed ports do not characterize SQLite or process adapter memory.

[Raw measurements and profiles](performance-data/2026-10-30-hosted-lazy-workspace/) include SHA-256 manifests. Massif, build JSONL and stderr are gzip-compressed byte-exact; metadata and measurement JSON remain directly readable. No performance exception is accepted by this report.
