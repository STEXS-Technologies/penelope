# Parallel linear saga follow-up: 2026-10-30

The [initial six-binary comparison](performance-decisions-2026-10-30.md) reported
a 12.9% slower candidate median for parallel linear saga, with wide overlapping
ranges. This follow-up adds a same-revision control and hardware counters to
investigate that observation. It does not clear the performance release gate.

The measured `parallel_linear_saga.rs` and executor `engine.rs` files are
unchanged from the pinned v0.2.0 baseline. No package already present in the
baseline Cargo.lock changed version. Source inspection does not establish
identical generated code or equivalent machine performance.

## Method and retained evidence

Same Ryzen 9 7950X host, Rust 1.99.0, benchmark datasets and four-CPU affinity
(0,1,2,3) as the initial comparison. The selected binary reports four workers and
400,000 total operations. Each campaign has twenty paired rounds after one
unrecorded warmup per binary, alternating revision order. Each process starts
fresh; there is no concurrent benchmark build in the measured loop. Host CPU
frequency, competing workloads and OS caches remain uncontrolled.

Two campaigns ran sequentially:

- **Self-control:** independently archived and built copies of baseline
  `edcc5b423e4919f51e4aa2cb820d9de68a684145`, labeled before and after.
  [Metadata](performance-data/2026-10-30-parallel-followup/parallel-self-control/metadata.json),
  [all 40 process records](performance-data/2026-10-30-parallel-followup/parallel-self-control/measurements.json),
  [summary](performance-data/2026-10-30-parallel-followup/parallel-self-control/summary.json).
- **Candidate comparison:** baseline versus
  `dd9d2628bf73a658d74b2ac417aa6d8015e3d1b3` (the exact full revision is
  authoritative in the linked metadata). Candidate Rust crate sources are
  unchanged from the initial report's `1aedbae` revision.
  [Metadata](performance-data/2026-10-30-parallel-followup/parallel-counter-comparison/metadata.json),
  [all 40 process records](performance-data/2026-10-30-parallel-followup/parallel-counter-comparison/measurements.json),
  [summary](performance-data/2026-10-30-parallel-followup/parallel-counter-comparison/summary.json).

Linux perf records task-clock, instructions, cycles, context switches and CPU
migrations. Counters cover the GNU time wrapper plus benchmark and inherited
threads. GNU time wall/RSS cover the benchmark process and its threads. Counter
values include userspace and kernel work; they are not counts of an isolated
engine function. All retained counters report 100% running time, without
multiplexing. Raw context-switch and migration counts remain in every record.

The runner rejects unavailable, missing, duplicate, non-finite or disabled
counters. The analysis requires matching operation counts and pairs samples by
round number. It reports a geometric mean of paired after/before ratios and an
exploratory 95% percentile bootstrap interval over those pairs (10,000 resamples,
seed 3030). No noninferiority margin or multiple-comparison adjustment was
specified. These intervals are descriptive and cannot establish release
acceptance or absence of a smaller regression.

## Results

Raw median values, before → after. Task-clock is aggregate CPU time in
milliseconds; instruction/cycle values are total counts per process workload.
The paired difference uses the geometric mean of paired ratios, so it can have
a different sign from the ratio of the two marginal medians.

| Campaign / metric | Before median | After median | Paired difference | Exploratory 95% interval |
| --- | ---: | ---: | ---: | ---: |
| Self-control ns/op | 73.79 | 74.59 | -0.99% | -4.44% to +2.13% |
| Self-control task-clock ms | 116.86 | 117.64 | +0.94% | -0.08% to +1.99% |
| Self-control instructions | 2,123,712,735 | 2,123,655,176 | -0.0019% | -0.0045% to +0.0007% |
| Self-control cycles | 613,502,350 | 617,727,752 | +0.95% | -0.05% to +1.99% |
| Candidate ns/op | 90.46 | 93.80 | +3.44% | -2.83% to +10.21% |
| Candidate task-clock ms | 121.08 | 124.54 | +2.99% | -0.81% to +6.77% |
| Candidate instructions | 2,123,825,404 | 2,123,852,143 | +0.0019% | -0.0053% to +0.0112% |
| Candidate cycles | 632,651,072 | 651,260,770 | +2.71% | -0.88% to +6.21% |

The 12.9% marginal median difference was not reproduced: the follow-up marginal
median delta is +3.70%. Instruction counts are nearly identical. Both campaigns
show timing variation, including the same-revision control, and the baseline's
median differs between campaigns. These observations support investigating host
scheduling, frequency, cache effects and generated-code layout. They do not
identify a cause or exclude a real smaller regression. No production code was
changed on the basis of this experiment. The performance concern remains open;
there is no accepted exception based on source equality or these intervals.

## Reproduce

Linux perf must be installed and permitted to collect these events. Other
requirements match the initial report. The runner can now select one or more
binaries with repeated `--benchmark` and collect counters with `--perf`.

```bash
python3 scripts/benchmark-decision-comparison.py target/parallel-self-control \
  --before edcc5b423e4919f51e4aa2cb820d9de68a684145 \
  --after edcc5b423e4919f51e4aa2cb820d9de68a684145 \
  --cpu 0 --parallel-cpus 0,1,2,3 --benchmark parallel_linear_saga --perf --runs 20
python3 scripts/benchmark-decision-comparison.py target/parallel-counter-comparison \
  --before edcc5b423e4919f51e4aa2cb820d9de68a684145 --after dd9d262 \
  --cpu 0 --parallel-cpus 0,1,2,3 --benchmark parallel_linear_saga --perf --runs 20
python3 scripts/summarize-decision-perf.py target/parallel-self-control
python3 scripts/summarize-decision-perf.py target/parallel-counter-comparison
```

Metadata pins the runner source hash for these measurements. The initial report
was generated with the earlier runner committed in `dd9d262`; its historical
source hash and results remain unchanged. The checked-in measurements files
combine each campaign's per-process JSON files without removing fields.
