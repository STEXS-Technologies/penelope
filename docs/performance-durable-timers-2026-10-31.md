# Durable timer workload — 2026-10-31

The new `durable_timer_workload` benchmark measures complete synthetic SQLite-backed timer workflows. In this run, about 60 µs of business computation per timer required **3.7–5.3 ms per timer** when initial process admission, timer scheduling, durable delivery, result persistence, and atomic input consumption were included. These are workload-specific durability costs, not lightweight orchestration overhead and not a universal Penelope slowdown.

No production runtime behavior changes in this addition. The purpose is to expose costs that the existing CPU probes and six-case application workload do not cover, and provide a stronger basis for subsequent optimization.

## What runs

Each observation opens a fresh database and prepares deterministic timers outside timing. The measured phases are:

1. **Schedule:** atomically admit a start input, initial outcome, and planned timer actions, then schedule every timer.
2. **Deliver:** call the public `drain_timers` service with the real SQLite claim, payload, inbox, and acknowledgement ports. The authorizer always authorizes; the trusted clock stays at logical time 100, with every timer due at 100. This is a benchmark fixture, not a wall-clock waiting or authorization-cost benchmark.
3. **Reopen, optionally:** close the worker/store and reopen the database before discovering any timer input.
4. **Consume:** repeatedly discover pending inputs through the public 64-input API, run the same deterministic business kernel as the direct baseline, persist each eight-byte business result, and atomically commit its outcome with the timer input. All timers are consumed, including the second discovery page for a 128-timer batch.

`managed_ns` is the exact sum of these phase durations. It excludes initial database creation/migration, fixture construction, final verification/teardown, and small gaps between the phase clocks. Individual phase medians need not sum to the median phase sum. This measures serial workflow work, not queueing, waiting for a due time, contention, process startup, wall-clock crash recovery, or a consuming application's external work.

After timing, every observation reopens the database and verifies exact legacy timer input identities and payload bytes, each persisted business result against a fresh computation, exact outcome history, successful duplicate commit retries, no pending inputs, and no remaining deliverable timers. Direct and managed computations must have equal checksums. Failures return nonzero and produce no verified row. Existing partial-batch fault/recovery tests cover interrupted delivery; this workload measures successful delivery and durable rediscovery without injecting a crash during timing.

The direct baseline runs only the same seeded CPU kernel and checksum accumulation. It has no durable storage, inbox, scheduling, or recovery guarantees. Both paths include seed extraction. Ordering alternates direct/managed by round. The kernel matches the existing application workload's recurrence; compiler barriers retain its computation. Comparing this baseline with durable execution deliberately measures the cost of the additional guarantees.

SQLite uses the adapter's WAL and `synchronous=FULL` configuration. No durability settings are relaxed.

## Observations

The runner used six process repeats, two rounds, batches 1/16/128, identifier widths 9/128, work sizes 0/65,536, and both reopen modes: **24 profiles, 288 verified workflows, 13,920 consumed timer inputs**. The work-zero profiles expose fixed costs; their percentage relative to near-zero computation is not useful as an overall application overhead estimate. All raw samples, including those profiles, are retained.

The table shows medians for 65,536 work units. Direct work is µs per timer; all remaining columns are ms per timer. Total includes the optional reopen phase, whose separate values are in the JSON summary.

| Batch | Identifier width | Reopen before consumption | Direct µs/timer | Schedule ms/timer | Deliver ms/timer | Consume ms/timer | Total ms/timer |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 9 | false | 59.73 | 1.358 | 2.332 | 1.308 | 4.998 |
| 1 | 9 | true | 59.73 | 1.391 | 2.268 | 1.455 | 5.284 |
| 1 | 128 | false | 59.73 | 1.396 | 2.237 | 1.285 | 4.925 |
| 1 | 128 | true | 59.73 | 1.411 | 2.273 | 1.472 | 5.328 |
| 16 | 9 | false | 60.01 | 0.649 | 1.757 | 1.324 | 3.734 |
| 16 | 9 | true | 60.01 | 0.657 | 1.800 | 1.346 | 3.863 |
| 16 | 128 | false | 59.98 | 0.702 | 1.855 | 1.446 | 4.046 |
| 16 | 128 | true | 59.97 | 0.699 | 1.865 | 1.360 | 3.912 |
| 128 | 9 | false | 60.42 | 0.645 | 1.896 | 1.415 | 3.927 |
| 128 | 9 | true | 60.41 | 0.631 | 1.936 | 1.409 | 3.992 |
| 128 | 128 | false | 60.34 | 0.707 | 2.055 | 1.457 | 4.277 |
| 128 | 128 | true | 60.39 | 0.714 | 1.984 | 1.499 | 4.187 |

Environment: AMD Ryzen 9 7950X 16-Core Processor, one pinned CPU (0), `btrfs /dev/nvme0n1p3[/home]`, stable Rust 1.99.0. Values are medians from this machine, with no confidence interval or cross-machine generalization. Configurations reverse order on alternating repeats. Builds, tests, allocation probes, and analysis did not overlap ordinary timing. Untimed verification still occurs between observations and may affect subsequent machine state.

The result supports targeting durable operations in future investigations. It does not establish that any particular batching or caching change is safe or faster. The previously rejected hexadecimal optimization remains rejected; it was not used in this workload and was not compared against this baseline.

## Run and verify

```sh
python3 scripts/benchmark-durable-timers.py --output target/my-durable-timers
cargo bench -p penelope-executor --bench durable_timer_workload -- --batch 128 --width 128 --work 65536 --rounds 2 --reopen true
python3 docs/verification-data/2026-10-31-durable-timers/verify.py
```

The runner requires Linux `taskset`, `findmnt`, and `/proc/cpuinfo` for pinning and environment capture, consistent with the existing benchmark tooling. The Rust benchmark itself uses the portable SQLite adapter and accepts bounded batch/work/round parameters; its default invocation is a small correctness smoke run. Initial filesystem setup follows `TMPDIR` so measurements can intentionally target a chosen filesystem.

[Archived evidence](verification-data/2026-10-31-durable-timers/) includes every sample, exact summaries, compiler/environment and source/binary hashes, runner source, qualification logs, and negative controls. The verifier checks provenance, recomputes the complete profile matrix and medians, rejects malformed observation sets, and checks fault/qualification records. It does not rerun historical timing or claim that current hardware will reproduce exact values.

## Qualification

Workspace tests, executor/process-runtime tests on Rust 1.85.0, strict workspace Clippy over all targets/features, the MSRV executor all-target check, formatting, layer checks, and the graph update passed. A separate Rust 1.85.0 execution consumed 256 timers across two 128-timer maximum-width/reopen observations. Ten invalid benchmark configurations were rejected.

Four temporary workload mutations were rejected: writing the wrong business result, stopping after the first 64 discovered inputs, substituting an expected timer identity, and returning a trusted clock before the timers were due. The source files were restored exactly and the restored release workload passed again. These controls validate the workload's checks; they are not injected production failures during the measured matrix. The archive verifier also rejects missing, duplicate, unverified, phase-inconsistent, unbalanced, checksum-inconsistent, and truncated-consumption observation sets.
