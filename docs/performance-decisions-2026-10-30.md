# Saga and contract comparison: 2026-10-30

This comparison measures the six existing pure saga/contract benchmark binaries
against the blueprint's pinned v0.2.0 baseline. It supplements the separate
[SQLite integration baseline](performance-sqlite-2026-10-30.md). It does not
qualify task supervision, persistence or protected execution performance.

Baseline: `edcc5b423e4919f51e4aa2cb820d9de68a684145`.
Candidate: `1aedbaeadf2a9a8f1f657a05b97d06921206c30d`.
The benchmark sources and their built-in datasets are byte-identical between
these revisions; the runner checks all six SHA-256 hashes before building.
Each revision uses its own committed Cargo.lock. Both lock hashes, benchmark
hashes, executable hashes, runner hash, compiler details and build environment
are recorded in [metadata.json](performance-data/2026-10-30-decisions/metadata.json).

## Method

Rust 1.99.0, optimized Cargo bench profile, AMD Ryzen 9 7950X, Linux x86-64.
Serial workloads are pinned to CPU 0; parallel workloads and all their threads
are restricted to CPUs 0,1,2,3. Parallel benchmarks report four workers. The CPUs
are affinity-constrained, without exclusive host CPU isolation or controlled
frequency. Benchmark processes run sequentially, with no concurrent builds in
the measured loop.

The runner archives each exact revision into a private temporary directory,
builds with `--locked` and separate explicit target directories, and runs one
unrecorded warmup process per binary. It then measures ten rounds, alternating
which revision runs first. Both revisions use the same fixed benchmark order.
Every sample starts a new process. OS caches are uncontrolled; these are warmed
runs, without a cold-cache claim. No SQLite or filesystem sync occurs in these
pure workloads.

[measurements.json](performance-data/2026-10-30-decisions/measurements.json)
contains all 120 measured process records, preserving their original result
filenames, stdout, stderr, operation counts, internal elapsed times, and GNU time
wall/user/system time and peak RSS. The two-operation contract binary produces
two observations per process. Values below use internal benchmark duration
divided by its reported operation count. Parallel values are aggregate elapsed
time per operation across four workers.

## Results

Nanoseconds per operation, median with observed minimum–maximum in parentheses.
The delta compares candidate median to baseline median; a positive delta is slower.
These are descriptive samples, without confidence bounds or a causal attribution.

| Operation | Baseline ns/op | Candidate ns/op | Median delta |
| --- | ---: | ---: | ---: |
| Canonical DTO encoding | 350.1 (336.5–536.2) | 330.9 (323.2–351.0) | -5.5% |
| Bounded outcome-log append | 541.0 (505.5–855.1) | 525.6 (513.1–640.0) | -2.8% |
| Graph saga start/result/result | 986.3 (964.8–1532.0) | 1015.7 (997.0–1603.8) | +3.0% |
| Linear saga start/result/complete | 275.4 (268.7–422.6) | 280.5 (270.5–414.2) | +1.9% |
| Parallel graph saga | 401.7 (299.9–510.2) | 406.7 (281.1–455.3) | +1.2% |
| Parallel linear saga | 99.9 (71.9–166.7) | 112.8 (74.0–136.6) | +12.9% |
| Linear saga replay | 423.3 (410.3–630.0) | 431.3 (411.3–570.9) | +1.9% |

Whole-process peak RSS and wall-time ranges include startup and all threads.
GNU time wall seconds have hundredth-second resolution; use internal benchmark
times for operation costs. RSS is not incremental library memory.

| Binary | Baseline RSS KiB | Candidate RSS KiB | Baseline wall s | Candidate wall s |
| --- | ---: | ---: | ---: | ---: |
| contract_throughput | 3952–4500 | 3944–4504 | 0.03–0.05 | 0.03–0.04 |
| graph_saga | 1920–2232 | 1928–2232 | 0.09–0.15 | 0.10–0.16 |
| linear_saga | 1860–2196 | 1920–2232 | 0.02–0.04 | 0.02–0.04 |
| parallel_graph_saga | 2116–2228 | 2120–2252 | 0.12–0.20 | 0.11–0.18 |
| parallel_linear_saga | 1944–2232 | 2176–2244 | 0.03–0.06 | 0.03–0.05 |
| replay_saga | 1960–2216 | 1920–2228 | 0.41–0.63 | 0.41–0.57 |

The slower saga medians remain unresolved for release qualification, especially
the parallel linear case. Wide overlapping ranges prevent attributing the
observed changes to library execution from this campaign alone. There is no
accepted performance exception or release pass based on these results. A more
controlled follow-up must resolve the differences before claiming the required
absence of unexplained regressions.

Allocation counts, task supervisor memory/stack high-water marks, task lifecycle
costs, cold-cache runs and real worker dispatch-to-ACK remain separate missing
measurements. This report makes no durable throughput, latency SLO or
zero-allocation claim.

## Reproduce

Requires Python 3.12 or later, Git, Rust 1.99.0, GNU time and Linux taskset.
Choose CPUs inside the current affinity mask and a new output directory.
The output contains metadata and one JSON file per measured binary/process.
The checked-in measurements file combines those files without removing fields.

```bash
python3 scripts/benchmark-decision-comparison.py target/decision-comparison \
  --before edcc5b423e4919f51e4aa2cb820d9de68a684145 \
  --after 1aedbaeadf2a9a8f1f657a05b97d06921206c30d \
  --cpu 0 --parallel-cpus 0,1,2,3 --runs 10 --toolchain 1.99.0
```

The [parallel follow-up](performance-parallel-followup-2026-10-30.md) adds a
same-revision control, hardware counters and paired uncertainty analysis. It
does not resolve the performance concern or replace these original samples.
