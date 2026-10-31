# Hosted borrowed-adapter reconstruction — 2026-10-30

Candidate `7d2a2b9` reconstructs the projection through borrowed journal/runtime ports before constructing the owning controller. It retains scoped poll and lazy-dispatch buffer frames. Baseline: `edcc5b423e4919f51e4aa2cb820d9de68a684145`. The sampled lifecycle stack regression reported for earlier candidates is removed at both tested capacities. Native marginal medians are also below baseline, with substantial timing variation at 64 KiB. This is evidence for these workloads, not a universal speed or static stack guarantee.

The [same fixture and method](hosted-task-memory-method.md) use CPU 0, Rust 1.99.0, identical capacity-specific inputs, three alternating pairs and one million complete native lifecycles per invocation. There were no concurrent agent builds/stress campaigns during measurement. OS caches, frequency and unrelated host activity remain uncontrolled.

| Capacity | Path | Baseline wall seconds | Candidate wall seconds | Median change |
| ---: | --- | --- | --- | ---: |
| 4,096 | pure_checkpoint_recovery | 2.55, 2.42, 2.51 | 2.35, 2.44, 2.45 | -2.8% |
| 4,096 | effect_reconciliation_recovery | 3.56, 3.47, 3.44 | 3.22, 3.49, 3.19 | -7.2% |
| 65,536 | pure_checkpoint_recovery | 5.16, 6.46, 6.37 | 6.05, 5.78, 4.91 | -9.3% |
| 65,536 | effect_reconciliation_recovery | 6.25, 8.45, 8.14 | 7.50, 7.90, 5.73 | -7.9% |

The 64 KiB first pair is slower in the candidate and subsequent pairs are faster. Three pairs and a ratio of marginal medians do not establish statistical noninferiority or a precise causal speedup. No throughput promise is derived from this table.

Maximum retained Massif stack samples, identical across each set of three runs:

| Capacity | Whole-process case | Baseline bytes | Candidate bytes |
| ---: | --- | ---: | ---: |
| 4,096 | stack_baseline | 68,640 | 55,952 |
| 4,096 | stack_positive_control | 134,008 | 121,320 |
| 4,096 | pure_checkpoint_recovery | 85,056 | 66,064 |
| 4,096 | effect_reconciliation_recovery | 85,056 | 66,064 |
| 65,536 | stack_baseline | 68,640 | 55,952 |
| 65,536 | stack_positive_control | 134,008 | 121,320 |
| 65,536 | pure_checkpoint_recovery | 202,304 | 188,944 |
| 65,536 | effect_reconciliation_recovery | 202,304 | 188,944 |

Lifecycle samples are 18,992 bytes below baseline at 4 KiB and 13,360 bytes below at 64 KiB. These are whole-process samples including startup, fixture moves and compiler frame placement, not worst-case static bounds or isolated supervisor frame sizes. The fixed journal is deliberately large; arbitrary consumer-owned adapters can change memory costs.

All native lifecycle intervals assert zero measured allocations, reallocations, deallocations and requested bytes after positive calibration. Expected output, starts/fences and checkpoint ACKs are checked. Both revisions retain the same supervisor object layouts: 13,656 bytes with the fixed fixture ports and 416 with zero-sized ports. SQLite/process adapters are outside this harness. Native and instrumented measurements run in separate processes.

[Raw evidence](performance-data/2026-10-30-hosted-borrowed-restore/) includes 48 Massif profiles, allocator assertion output, native time/RSS, fixture/build source and pinned tool/revision metadata with byte-exact archive hashes. Massif, JSONL and stderr are gzip-compressed; measurements and metadata remain directly readable.

The earlier hosted sampled-stack regression is no longer observed in this candidate. Remaining release work includes the independent core/saga performance review, cold/warm qualification, explicit memory-bound scope, full adversarial campaign and final requirement audit.
