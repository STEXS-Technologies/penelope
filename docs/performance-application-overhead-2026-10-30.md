# Direct task execution versus in-memory saga overhead — 2026-10-30

A [subsequent identifier-storage optimization](performance-inline-identifiers-2026-10-30.md)
further reduces the engine cost. The figures below are retained as the earlier
linear-clone stage, not the latest branch measurements.

Penelope's one-step in-memory linear engine adds approximately **0.24–0.26 µs**
in this synthetic CPU-task fixture after the optimization. The empty-task paired
median added time fell from **270.02 ns** to
**245.75 ns**, a **8.99% reduction**.
This is an application-task comparison, distinct from the earlier graph benchmark's
release-to-release regression/improvement percentages.

## Scope

The exact same non-inlined deterministic CPU kernel, runtime input and checksum
run directly and inside `start` → task → `apply_action_result`. The latter includes
identifier clones, definition validation, action/projection construction, completion
validation and dropping the owned decisions. Each timed path checks errors; results
must match, including untimed per-input warmups and timed whole-loop checksums.

This measures the pure in-memory one-step happy path. It excludes persistence,
authorization ports, event recording/replay, durable inbox/outbox delivery,
worker IPC/startup, isolation, effects, failures and recovery. **It is not the
end-to-end overhead of reliable execution or a specific consuming application.**
No reliability checks or durable operations were disabled to obtain these results;
those services are outside this benchmark's scope in both revisions.

## Fixed confirmation results

| CPU task | Direct median | With Penelope median | Paired added-time median | Before overhead | After overhead |
| --- | ---: | ---: | ---: | ---: | ---: |
| 64 kernel iterations | 44.02 ns | 283.80 ns | 240.70 ns | 630.57% | **556.58%** |
| 4,096 kernel iterations | 3785.45 ns | 4028.23 ns | 242.74 ns | 7.06% | **6.39%** |
| 65,536 kernel iterations | 60753.73 ns | 61028.32 ns | 263.40 ns | 0.48% | **0.43%** |

Overhead is computed separately for each adjacent direct/managed measurement:
`100 × (managed_ns / direct_ns − 1)`, then the 160 paired percentages are
summarized by their median. Added time is similarly paired before taking its
median. Consequently, displayed marginal medians need not reproduce the paired
columns exactly. The empty-task direct path is approximately 1 ns; its enormous
percentage is not a useful application estimate, so its absolute added time is
reported instead. The raw summary retains that percentage too.

The ~61 µs task has a measured median overhead of **0.43%**, but its optimized
individual paired percentages range from **−1.61% to +2.67%**. Scheduling/frequency
noise is significant relative to such a small difference. This does **not** establish
a universal or worst-case ≤1% guarantee. The smaller ~3.8 µs task still has **6.39%**
median overhead, and a ~44 ns task has **556.58%**. Work duration and integration
services determine the percentage; there is no single Penelope overhead constant.
The long-task total median did not improve between revisions; the empty-task and
shorter-task measurements provide the clearer evidence of reduced engine cost.

## Method and provenance

- Baseline `f00b694833a6cc8691fd86544b78fb6079088927`: prior qualified production
  code plus the new comparison benchmark, before the linear optimization.
- Optimized `8a9e298940d68de3d3d19c152a9bbef7f0fc54a0`; benchmark source and
  locked dependencies are identical. Metadata records source and binary SHA-256.
- Rust 1.99.0, release/bench profile, Ryzen 9 7950X, Linux, affinity CPU 0.
  CPU frequency and other host processes were not controlled. No local builds,
  tests or fuzzing overlapped the fixed confirmation campaign.
- Four process pairs with alternating revision order. Each process has 40 rounds
  per work size, alternating direct/managed order; 100 warmup/check inputs per
  size. Fixed iterations: 100,000 / 100,000 / 2,000 / 128 for work 0 / 64 /
  4,096 / 65,536. All **1,280 confirmation records** are retained.
- Initial before/after pilot runs are retained separately and excluded from this
  table. No additional reruns selected for favorable timing.

Reproduce either pinned revision with a separate checkout and target directory:

```sh
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo +1.99.0 bench \
  -p penelope-executor --bench application_overhead --no-run --locked
# Use the executable reported by Cargo (target/release/deps/application_overhead-*).
taskset -c 0 /path/to/that/executable > samples.jsonl
```

The source is [application_overhead.rs](../crates/penelope-executor/benches/application_overhead.rs).
[Raw evidence and manifest](performance-data/2026-10-30-application-overhead/manifest.json),
[environment/provenance](performance-data/2026-10-30-application-overhead/metadata.json)
and [unrounded summaries](performance-data/2026-10-30-application-overhead/summary.json)
retain the observations and checksums.

## Implementation and correctness checks

`terminal_projection` copies only the retained definition metadata, retry timing,
history and scalar fields. It previously cloned the whole projection and then
discarded clones of fields supplied explicitly, including active action and history.
`issued_ids_after` reserves one extra element before copying the existing history,
avoiding its immediate growth allocation. The one-step benchmark exercises the
terminal change; it does not isolate the history-growth improvement.

An independent frozen-expression property compares every terminal projection field
and appended action-history contents: **10,000 cases on Rust 1.99.0 and 10,000 on
MSRV 1.85.0**. Stable workspace all-target/all-feature tests, MSRV executor
all-target/all-feature tests, workspace doctests, strict workspace Clippy, fmt,
layer checks and **100,000 fuzz_linear_engine inputs (seed 4)** pass. Graphify was
updated. Compressed local logs are included with the raw evidence. Earlier full
release qualification evidence remains pinned to its documented revisions; these
are additional scoped checks, not a rerun of its entire multi-hour campaign.

[All seven remote CI jobs](https://github.com/STEXS-Technologies/penelope/actions/runs/41132374645)
passed on the optimized source `8a9e298`, including stable/MSRV, portable core,
optional actual StateChronicle conformance, bounded fuzzing and supply-chain checks.
