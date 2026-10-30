# Hosted scoped-buffer frames — 2026-10-30

Candidate `0fac62a` keeps hosted polling and lazy dispatch storage in separate call frames. Baseline: `edcc5b423e4919f51e4aa2cb820d9de68a684145`. This corrects the 64 KiB sampled stack regression observed in the [lazy-workspace candidate](performance-hosted-lazy-workspace-2026-10-30.md), but the 4 KiB lifecycle sample and native timing remain above baseline. Qualification remains open.

The [unchanged fixture and method](hosted-task-memory-method.md) use CPU 0, Rust 1.99.0, identical capacity-specific inputs, three alternating pairs and one million complete native lifecycles per invocation. No concurrent agent build/stress work ran. OS caches, frequency and unrelated host activity remain uncontrolled.

| Capacity | Path | Baseline wall seconds | Candidate wall seconds | Median change |
| ---: | --- | --- | --- | ---: |
| 4,096 | pure_checkpoint_recovery | 2.48, 2.43, 2.48 | 2.73, 2.79, 2.74 | +10.5% |
| 4,096 | effect_reconciliation_recovery | 3.48, 3.54, 3.45 | 3.99, 3.75, 3.85 | +10.6% |
| 65,536 | pure_checkpoint_recovery | 5.10, 5.15, 5.05 | 5.18, 5.19, 5.27 | +1.8% |
| 65,536 | effect_reconciliation_recovery | 6.23, 6.55, 6.37 | 6.50, 6.39, 6.39 | +0.3% |

Maximum retained Massif stack samples, identical across the three pairs:

| Capacity | Whole-process case | Baseline bytes | Candidate bytes |
| ---: | --- | ---: | ---: |
| 4,096 | stack_baseline | 68,640 | 68,640 |
| 4,096 | stack_positive_control | 134,008 | 134,008 |
| 4,096 | pure_checkpoint_recovery | 85,056 | 98,672 |
| 4,096 | effect_reconciliation_recovery | 85,056 | 98,672 |
| 65,536 | stack_baseline | 68,640 | 68,640 |
| 65,536 | stack_positive_control | 134,008 | 134,008 |
| 65,536 | pure_checkpoint_recovery | 202,304 | 201,632 |
| 65,536 | effect_reconciliation_recovery | 202,304 | 201,632 |

The 4 KiB lifecycle difference is 13,616 bytes; the 64 KiB lifecycle sample is 672 bytes below baseline. Whole-process stack includes startup, fixture ownership/moves and compiler frame placement. These samples are not static worst-case bounds or an isolated supervisor frame size. The next experiment separates replay through borrowed adapters from constructing the owning controller.

All native lifecycle intervals assert zero allocation, reallocation, deallocation and requested bytes after calibration, correct output, starts/fences and checkpoint ACKs. Supervisor layouts remain 13,656 bytes with fixture ports and 416 with zero-sized ports. These fixed ports do not measure SQLite or process adapter memory. Native durations and instrumented stack samples come from separate processes. The table is not a confidence interval or an accepted performance exception.

[Raw evidence](performance-data/2026-10-30-hosted-scoped-frames/) retains profiles, time/RSS/stdout, source, build logs and metadata with byte-exact archive hashes. Massif, JSONL and stderr are gzip-compressed; measurements and metadata remain directly readable.
