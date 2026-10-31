# Worker protocol allocation and admission follow-up — 2026-10-31

Worker scope hashing now uses a fixed stack array instead of a temporary heap
buffer. Valid protocol bytes stay identical. The worker also rejects the pinned
definition's tighter input/checkpoint limits and schema zero before allocating or
reading payloads, even when the caller's transport limits are looser.

## Measured scope

Baseline `f6d7766` contains the new benchmark with the previous production code;
candidate `2e3a768` contains the optimization and regression checks. The benchmark
and shared fixture are identical at both revisions. Results cover in-process
protocol helpers, excluding process startup, SQLite, fencing and worker execution.
They do **not** establish a new application-wide overhead percentage.

20 alternating revision pairs, one warmup per revision, Rust 1.99.0 bench profile,
CPU 0, Ryzen 9 7950X/Linux. Other host workloads, frequency and caches are
uncontrolled. Nanoseconds per operation:

| Operation | Before median | After median | Time change |
| --- | ---: | ---: | ---: |
| completion proposal | 110.65 | 110.16 | -0.45% |
| checkpoint proposal | 110.87 | 110.09 | -0.71% |
| valid input-only request | 2040.60 | 2026.15 | -0.71% |
| request exceeding pinned input limit | 1984.96 | 1948.11 | -1.86% |

The descriptive paired bootstrap intervals for proposal time are below zero;
request intervals include zero. These small wall-time differences are exploratory,
not universal speed guarantees. Across the whole four-phase process, median
instructions decrease 2.58%, cycles 1.25%, and task-clock 1.32%. Those counters
cannot be attributed to individual phases.

## Reliability and allocation evidence

- A reader that refuses payload access proves invalid pinned input/checkpoint
  lengths and schema zero are rejected after metadata and before payload I/O.
  The new test fails on the baseline (`Runtime` from attempted payload access)
  and passes on the candidate (`Corrupt` without access). All three cases use
  small buffers; the test does not provoke large allocations.
- 10,000 varied scopes match an independent legacy heap-based encoding oracle.
  Existing proposal field/error, request metadata and checkpoint-ack contracts
  also pass. No public API, v1 wire encoding or dependency changes are introduced.
- An isolated counting allocator verifies zero allocations for either proposal,
  two for a valid input-only request and one for a pinned-input-limit rejection,
  after fixture setup. Rust 1.85 and 1.99 both pass. These exact counts apply to
  those fixtures; requests carrying checkpoints allocate additional payload storage.
- Stable workspace tests (all targets/features, excluding fuzz executable targets),
  doctests, documentation, strict workspace Clippy, formatting and layer checks
  pass. Runtime all-target/all-feature tests pass on Rust 1.85. Protocol fuzzing
  completes 100,000 iterations with seed 7. The graph was updated.

Historical full release campaigns, package archives and portable stack profiles
retain their original pinned revisions. This change touches the hosted worker
helper, not the allocation-free portable core. The scope-hash temporary is now a
159-byte stack array; this is not a measured whole-worker stack bound.

## Reproduction and retained evidence

[Raw evidence](performance-data/2026-10-31-worker-protocol/evidence.json) records
source revisions and check scope. The directory retains all 40 measured processes,
benchmark environment metadata, summary statistics, command logs, old-source
regression failure, runner copies and SHA-256 hashes. The wrapper only registers
`worker_protocol` with the existing comparison runner. Run it from `target/` in
the repository so its relative runner path resolves:

```sh
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= python3 target/worker-protocol-comparison.py \
  target/worker-protocol-comparison-new --before f6d7766 --after 2e3a768 \
  --cpu 0 --parallel-cpus 0,1,2,3 --runs 20 --benchmark worker_protocol --perf
```

[Source CI](https://github.com/STEXS-Technologies/penelope/actions/runs/41143074193)
passes all seven jobs; status is retained in the evidence manifest.
