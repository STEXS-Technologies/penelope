# Linux worker dispatch baseline — 2026-10-30

The real static Linux x86_64 worker now has a reproducible latency benchmark
using the actual SQLite task journal, persistent generation authority and Linux
runtime. This is a new optional adapter baseline, not a before/after claim against
v0.2, which had no equivalent process runtime.

Each task uses input `[5, 1, 2, 3]`: the worker proposes an eight-byte checkpoint,
waits for its ACK, then completes with the little-endian integer 6. Each sample
requires exactly one start and one checkpoint ACK, verifies checkpoint/output
bytes, and replays the durable task history to the same completed state. There
are two profiles: a fresh database/registry/runtime for each task, and reuse of
one initialized runtime/store with distinct task scopes. Reuse has one retained
warmup excluded from percentiles. Each run measures 64 tasks per profile, plus
that warmup: 129 actual workers per run.

All durations below are milliseconds. Ranges span the three runs of the campaign
serialized after the agent's workspace checks finished; background OS activity
and CPU frequency remain uncontrolled. These are ranges of per-run percentiles,
not confidence intervals or pooled percentiles.

| Profile / interval | p50 range | p95 range | p99 range |
| --- | ---: | ---: | ---: |
| Fresh: intent commit | 0.571–0.603 | 0.617–0.665 | 0.720–0.784 |
| Fresh: dispatch to start return | 3.174–3.242 | 3.471–3.563 | 3.671–4.336 |
| Fresh: dispatch to host checkpoint ACK | 3.839–3.948 | 4.178–4.267 | 4.356–5.478 |
| Fresh: dispatch to durable completion and fence | 5.568–5.770 | 6.004–6.108 | 6.882–7.329 |
| Reused: intent commit | 0.662–0.670 | 0.723–0.790 | 1.877–2.025 |
| Reused: dispatch to start return | 2.923–2.953 | 3.122–3.388 | 4.330–5.645 |
| Reused: dispatch to host checkpoint ACK | 3.641–3.687 | 3.880–4.066 | 5.050–6.385 |
| Reused: dispatch to durable completion and fence | 5.576–5.609 | 6.419–6.891 | 7.038–8.241 |

With only 64 measured samples, nearest-rank p99 is the sample maximum. Larger
campaigns are needed to characterize tails. Reuse is a different lifecycle profile;
the table does not establish a general reuse speedup.

The timer begins immediately before `TaskSupervisor::advance`: it includes the
Started history commit, input read, durable generation reservation, registry
identity fsync, fork/exec and start-frame write. Intent persistence is timed
separately. The ACK endpoint is successful completion of the **host ACK write**
after the checkpoint commit, not an independently acknowledged worker receipt.
Subsequent completion proves the fixture read its ACK. The completion interval
includes durable completion and runtime fencing. Post-measurement verification
and history replay are outside these intervals. Polling yields between pending
observations and uses monotonic whole-second task ticks. This is one polling
strategy, not an application-independent latency guarantee.

The runner pins parent and worker to CPU 0, uses Rust 1.99.0 and the locked bench
profile, and compiles the same freestanding C fixture before timed processes.
It records source, worker, runner, binary and lock hashes; the worker code digest
must match the separately built bytes in every run. Storage is NVMe-backed btrfs
with the journal's required WAL/FULL profile and registry fsync enabled. There is
no fsync bypass. Fresh connections/images are **not** OS-page-cache-cold runs;
no cache eviction was performed. Runtime/store initialization is reported as a
separate `setup_ns` sample. Reused setup after warmup only retrieves owned ports
and must not be interpreted as database initialization latency.

Serialized process wall times were 1.16–1.17 seconds; GNU time maximum RSS was
5,568–5,632 KiB. This includes benchmark setup, validation and teardown, excludes
worker compilation, and is not the sum of simultaneous parent/worker residency.
Allocation counts and stack high-water use are not measured by this benchmark.

An earlier campaign overlapped the start of workspace verification. Its raw
results are retained as a separate, contended context: fresh-profile p99 host
ACK ranged from 39.434 to 75.318 ms and completion reached 118.744 ms. Those
outliers were not filtered. This overlap was not a controlled load experiment;
it does not identify the cause of individual delays. Neither campaign establishes
an SLA, throughput claim, or a release performance pass.

Reproduce into a new output directory after other agent verification commands
have finished:

```sh
python3 scripts/benchmark-worker-dispatch.py /tmp/worker-dispatch \
  --data-parent /path/to/measured/filesystem --cpu 0 --runs 3 --samples 64
```

[Serialized raw results](performance-data/2026-10-30-worker-dispatch-serialized/)
and [overlapping-verification results](performance-data/2026-10-30-worker-dispatch/)
retain every task sample, warmup, percentile, resource report and build/host
metadata. The source changes are recorded relative to `1c1b9b2`; hashes identify
the exact new benchmark and runner. The reader validates sample identity,
chronological endpoint order, worker bytes and every percentile against raw
samples; malformed-report regression tests run in normal Python CI. The normal
workspace all-target suite runs a small benchmark smoke case on Linux x86_64.

Cold OS-cache measurements, hosted allocation/stack qualification, before/after
hosted supervision comparison and the existing saga performance concern remain
open release evidence requirements.
