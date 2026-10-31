# Durable timer storage and CPU-affinity audit — 2026-10-31

A matched CPU-budget check supports the earlier durable workload result: allowing two CPUs instead of one did not demonstrate a speedup for a 128-timer, maximum-width, work-zero workflow on Btrfs. A syscall diagnostic also confirms substantial filesystem synchronization cost. No runtime change or new speedup is shipped in this audit.

## Matched ordinary runs

The committed durable timer workload binary was unchanged. Both configurations used the same explicit Btrfs temporary-database directory, 128 timers, 128-byte identifiers, work zero, no reopen before consumption, and the same successful-delivery and final-reopen verification. The only varied condition was the process's allowed CPU set: logical CPU 0 versus logical CPUs 0 and 1. The application still ran one serial business thread and one SQLite worker; two allowed CPUs did not mean two application workers.

Two campaigns each used six balanced ABBA/BAAB blocks, four processes per block, and two alternating direct/managed rounds per process: **96 verified workflows and 12,288 consumed timers**. No builds, tests, analysis, or profiling overlapped these ordinary measurements.

| Campaign | One-CPU median ms/timer | Two-CPU median ms/timer | Paired geometric change, two versus one | Bootstrap 95% interval |
| --- | --- | --- | --- | --- |
| 0 | 4.193 | 4.233 | +1.44% | −0.98% to +4.43% |
| 1 | 4.288 | 4.199 | +2.54% | −0.33% to +6.85% |

The point estimates use paired block log ratios, not the ratio of the pooled medians; their direction can differ. Intervals use 20,000 deterministic bootstrap resamples of six blocks and are unadjusted for multiple comparisons. Neither interval supports a gain or regression. These data constrain this specific workflow and CPU-budget question; they do not establish that CPU affinity never matters elsewhere.

## Syscall diagnostic and temporary-directory correction

After ordinary timing finished, a separate `strace -f -c -w` run on the same Btrfs volume and one CPU recorded **794 fsync calls, averaging 571 µs**, for the whole process. The trace includes initial database setup and final verification, not only the measured phases. Tracing perturbs execution, and futex and filesystem wait durations can overlap across threads. The table's syscall percentages must not be interpreted as exclusive percentages of application overhead. The trace supports investigating durable write boundaries; it does not quantify the benefit or safety of changing them.

An earlier diagnostic accidentally inherited the default `/tmp` directory, which is tmpfs on this machine. Its inexpensive fsync calls initially suggested that worker scheduling might dominate. Inspection found `TMPDIR` unset and `/tmp` mounted as tmpfs. This was a mismatch with the committed workload runner, which explicitly places databases on Btrfs. The subsequent Btrfs runs corrected that mismatch. The earlier workload measurements remain valid.

The exploratory tmpfs affinity data and trace are preserved, with their provenance and limitations. They are not pooled with Btrfs results. The exploratory trace also used a different reopen option and inherited CPU affinity, and the two storage experiments were not interleaved; no precise cross-filesystem speedup is claimed. These distinctions matter because a correctly configured SQLite WAL/FULL benchmark on a volatile filesystem does not measure the same physical persistence costs as the Btrfs workload.

The committed workload runner already sets `TMPDIR` explicitly and records the output/database filesystem. No runner correction was required.

## Evidence

[Archived evidence](verification-data/2026-10-31-timer-storage-audit/) contains both ordinary observation sets, paired analyses and exact summaries, environment/source/binary metadata, and both instrumented traces and verified workload rows. Source hashes must match the committed workload harness. No production source files, SQL statements, transaction boundaries, synchronization settings, or public APIs changed.

```sh
python3 docs/verification-data/2026-10-31-timer-storage-audit/verify.py
```

The verifier checks archive integrity, exact ordinary-run grids, checksums and completed consumption, source provenance, syscall records, and independently recomputed paired summaries. Repeating timing requires a fresh output directory and the recorded benchmark binary/compiler sources; scripts refuse to overwrite observations. For any new diagnostic, set and record `TMPDIR` before interpreting storage timings.
