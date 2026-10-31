# Bounded linear definition validation — 2026-10-31

The measured 128-step fixtures take **82.76–98.39% less validation time** while
retaining zero allocations. These are definition-admission measurements, not a
new application-wide slowdown percentage. The existing one-step saga benchmark
measures +0.49% time, with essentially unchanged instructions and a paired timing
interval including zero.

## Implementation and behavior

Definitions with 1–16 steps retain pairwise duplicate-ID checking. Size dispatch
uses the empty/threshold branches on that common path. Larger admitted definitions
copy borrowed IDs into a fixed scratch array and sort only those references.
Strictly descending IDs already prove uniqueness, avoiding poor reverse-input
small-sort behavior. Original step order and owned identifiers remain unchanged.

Empty and over-limit definitions still fail before duplicate checking; duplicate
IDs still precede forward/compensation retry-limit errors. Public APIs, wire bytes
and dependencies are unchanged. The larger scratch is isolated in a helper marked
`inline(never)`: 128 reference elements occupy 1024 bytes on the measured 64-bit
host. This is not a whole-frame stack bound. The portable core and its historical
stack profile are unchanged.

## Fixed comparison

Baseline `22a1926` adds the benchmark and frozen pairwise oracle with unchanged
production code. Candidate `dd3ac8d` contains the final optimization. Benchmark
sources, fixtures and lockfiles match. Rust 1.99.0 bench profile, 20 alternating
revision pairs, one warmup per binary/revision, Ryzen 9 7950X/Linux. Serial CPU 0,
parallel CPUs 0/1/2/3; other host workloads, caches and frequency are uncontrolled.

The new binary measures 60 valid-definition fixtures: 10 sizes
(1/2/4/8/9/16/17/32/64/128), two ID lengths and three fixed orderings. The long IDs
share a 96-character prefix. Nanoseconds per validation for 128-step definitions:

| IDs | Ordering | Before median | After median | Time change |
| --- | --- | ---: | ---: | ---: |
| short | ascending | 17592.17 | 548.51 | -96.88% |
| short | descending | 17609.57 | 283.28 | -98.39% |
| short | permuted | 17429.32 | 2443.32 | -85.98% |
| long | ascending | 18987.37 | 756.34 | -96.02% |
| long | descending | 18958.42 | 449.26 | -97.63% |
| long | permuted | 18940.75 | 3264.72 | -82.76% |

The six 17-step fixtures improve by 41.59–84.65%. Across measured sizes above
16, reductions range from 41.59% to 98.39%. Sizes 1–16 retain mixed median changes
from -15.12% to +3.18%, without a small-definition speed claim. These three
orderings do not establish an all-input timing bound. [All 60 fixture results](performance-data/2026-10-31-linear-validation/definition-fixtures.csv)
include paired statistics and intervals.

Across the whole new binary, instructions decrease 73.14%, task-clock 76.63% and
cycles 76.82%. Those counters combine all 60 fixtures, their different iteration
counts, setup and printing; they cannot be attributed to individual sizes.

## Existing workload controls

All six existing binaries also run in the fixed comparison. Positive medians
remain disclosed; every control's descriptive paired timing interval includes
zero, which does not establish noninferiority.

| Existing operation | Median time change |
| --- | ---: |
| linear start/result/complete | +0.49% |
| graph start/result/result | +2.11% |
| replay | +0.79% |
| parallel linear | -10.79% |
| parallel graph | +10.45% |
| canonical DTO encoding | +1.32% |
| bounded outcome-log append | +0.34% |

Existing workload instructions differ by less than 0.01%. The parallel graph's
positive wall-time median has -0.26% task-clock and -0.73% cycles; the parallel
linear's negative wall-time median has +1.63% task-clock. These differences
reinforce workload/host timing limits rather than establishing a general library
speedup or regression. No cross-report absolute timing comparison is made.

## Pilot and verification

The retained five-pair pilot at `807052d` exposed a 17-step reverse-order regression
of 20.61% for short IDs and 29.23% for long IDs. The strict-descent uniqueness check
corrects that path: the final reverse-17-step changes are -84.65% and -81.72%.
The final comparison uses a fixed 20 pairs; pilot samples are not merged with it.

- Two generated properties execute 10,000 cases each on Rust 1.99 and 1.85:
  40,000 generated cases checked against the frozen independent pairwise oracle.
- Deterministic checks cover cutoff/boundary sizes, mixed-invalid precedence,
  source order and reverse-order forward/compensation retry errors.
- An isolated allocator executable verifies 42 fixture validations per toolchain:
  24 valid and 18 duplicate cases across four sizes, both ID lengths and ascending,
  descending and rotated inputs. All allocate zero times after setup.
- Final-source full workspace all-target/all-feature suites pass on both
  toolchains, excluding fuzz executable targets. Strict workspace Clippy,
  warning-denied docs, doctests, formatting, layer checks and graph update pass.
- `fuzz_linear_engine` completes 100,000 iterations with seed 10. This bounded
  encoded-input target complements the large typed-definition properties; it is
  not the complete historical release campaign.
- [All seven source CI jobs](https://github.com/STEXS-Technologies/penelope/actions/runs/41146524464)
  pass at `dd3ac8d`.

## Retained evidence

[Manifest](performance-data/2026-10-31-linear-validation/evidence.json) retains 290
measured process records: 10 pilot and 280 final. Deterministic archives preserve
original JSON bytes with member hashes, alongside summaries, compressed command
logs, the CI record, wrapper, comparison runner and statistics scripts. Extract
an archive and pass its directory to `summarize-validation-comparison.py` to
reproduce its summary. Bootstrap intervals are descriptive paired log-ratio
percentiles, using 10,000 resamples and seed 3030; they are not simultaneous
acceptance tests for all cases.

The wrapper only registers `definition_validation` with the existing comparison
runner. Copy it into `target/` in the repository, then use `--before 22a1926
--after dd3ac8d --runs 20 --cpu 0 --parallel-cpus 0,1,2,3 --perf` with a new output
directory. Historical package archives, full release campaigns and portable
memory bounds retain their original pinned revisions.
