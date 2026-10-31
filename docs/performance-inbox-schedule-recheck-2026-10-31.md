# Inbox change: scheduling follow-up — 2026-10-31

The [inbox retry optimization](performance-inbox-retry-fastpath-2026-10-31.md) had one supported scheduling-phase regression: campaign 1, batch 16, identifier width 9, +19.42% [1.28%, 44.54%]. This follow-up repeats that configuration with the exact same parent/candidate executables, without rebuilding or changing runtime sources.

The phase combines the initial atomic process commit with public timer scheduling. It does not call public `Inbox.accept`, so its change cannot be attributed directly to that method's new initial SELECT. Atomic commit still calls its admission helper inside the existing transaction; the helper extraction and executable layout are among possible influences. Storage and scheduling variability are also possible; this experiment does not identify a cause.

## Matched repeat

Two new campaigns each use twelve balanced ABBA/BAAB blocks, twice the blocks per campaign in the original experiment. Each process runs two alternating direct/managed rounds: **96 processes, 192 verified workflows, 3,072 consumed timers**. Configuration is fixed at batch 16, width 9, work 65,536, and no reopen before consumption. Database files remain on Btrfs, CPU affinity is CPU 0, and no builds, tests, analysis, or allocation probes overlap timing. Original observations are retained separately and are not pooled with this follow-up.

The existing workload verifies durable business results, timer input identity, consumption and replay after an untimed reopen on every observation. Geometric mean paired changes and 95% intervals use 20,000 deterministic bootstrap resamples of twelve blocks. Intervals are unadjusted for multiple comparisons; negative means less time.

| Campaign | Phase | Baseline ms | Candidate ms | Change | 95% interval |
| --- | --- | --- | --- | --- | --- |
| 0 | managed | 59.830 | 59.838 | 0.01% | [-1.43%, 1.36%] |
| 0 | schedule | 10.192 | 10.253 | 0.59% | [-0.49%, 1.69%] |
| 0 | delivery | 28.676 | 28.344 | -1.16% | [-3.19%, 0.76%] |
| 0 | consume | 20.945 | 21.217 | 1.30% | [-0.23%, 2.98%] |
| 0 | direct | 0.958 | 0.960 | 0.16% | [-0.02%, 0.38%] |
| 1 | managed | 60.559 | 59.358 | -1.98% | [-4.64%, 0.09%] |
| 1 | schedule | 10.548 | 10.257 | -2.76% | [-9.27%, 1.83%] |
| 1 | delivery | 28.619 | 28.104 | -1.80% | [-3.07%, -0.65%] |
| 1 | consume | 21.071 | 20.972 | -0.47% | [-2.03%, 1.12%] |
| 1 | direct | 0.959 | 0.957 | -0.21% | [-0.47%, 0.05%] |

Neither campaign supports a scheduling regression. Scheduling changes are +0.59% [−0.49%, 1.69%] and −2.76% [−9.27%, 1.83%]. Total workflow changes are +0.01% [−1.43%, 1.36%] and −1.98% [−4.64%, 0.09%], both inconclusive. One delivery comparison supports a gain; all other phase/control comparisons are inconclusive.

The original scheduling regression did not recur here. That weakens the evidence for a persistent penalty in this configuration; it does not prove zero cost, invalidate the original observation, establish a speedup across workloads, or explain its cause. No production change is justified by this repeat alone. The previously qualified inbox optimization remains unchanged.

## Evidence

[Archive](verification-data/2026-10-31-inbox-schedule-recheck/) contains all new observations, exact summaries, measurement and analysis scripts, executable hashes, environment metadata, and source hashes. Executable hashes must match the earlier inbox archive, which retains their build provenance and both runtime snapshots. Executables and databases are not committed.

```sh
python3 docs/verification-data/2026-10-31-inbox-schedule-recheck/verify.py
```

The verifier validates the complete paired grid, recomputes the exact summary, checks source/executable provenance, and requires five malformed observation datasets to be rejected. For fresh timing, obtain the two pinned executables using the earlier archive's build instructions, then run this archive's `measure.py --binaries <directory> --output <fresh-directory>` from the repository root. The executable directory must include its original `metadata.json`. Run `analyze.py public <output-directory>` afterward. Match compiler, CPU, and Btrfs placement; exact historical results are not guaranteed.
