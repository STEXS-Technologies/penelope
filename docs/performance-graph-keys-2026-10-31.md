# Bounded graph transition-key optimization — 2026-10-31

The fixed confirmation measures **13.68% less serial graph time** and **16.49%
less parallel graph time**, with approximately **11.9% fewer instructions** in
both workloads. Timing remains workload- and host-dependent: unchanged controls
also vary, and the first comparison's unfavorable controls are retained below.
This does not establish a new application-wide overhead percentage.

## Implementation and compatibility

`ProcessGraphDefinition::validate` records transition keys in a fixed per-source
mask rather than a `BTreeSet`. There are at most 128 steps and exactly three
outcome categories; each category has its own bit. Source IDs still use the
existing sorted index. Bounds, source and destination checks, duplicate-key
classification and reachability retain their ordering and behavior. No public
API, wire encoding or dependency changes are introduced.

This removes the transition-key tree's heap storage and declares a 128-byte
hosted stack mask. The sorted-ID vector and traversal allocations remain. No
zero-allocation or whole-frame stack bound is claimed. The portable core and its
historical stack profile are unchanged.

## Fixed confirmation

Baseline `99d9a3d`, candidate `005ea60`. Rust 1.99.0 bench profile; 40 alternating
revision pairs, one warmup per binary/revision, unchanged benchmark sources and
lockfiles. CPU 0 serial; CPUs 0/1/2/3 parallel, Ryzen 9 7950X/Linux. Other host
workloads, frequency and OS caches are uncontrolled. Values are nanoseconds per
operation as reported by each existing benchmark:

| Operation | Before median | After median | Time change |
| --- | ---: | ---: | ---: |
| graph start/result/result | 506.81 | 437.50 | -13.68% |
| parallel graph | 191.19 | 159.66 | -16.49% |
| parallel linear, unchanged control | 38.21 | 41.39 | +8.30% |

Serial graph instructions decrease 11.86%, task-clock 13.56% and cycles 13.85%.
Parallel graph instructions decrease 11.92%, task-clock 18.60% and cycles 17.97%.
The parallel-linear control's instructions change -0.005%, task-clock -0.25%
and cycles +0.002%. Its wall-time change is therefore not accompanied by an
increase in measured CPU work; this is evidence of timing variability, not proof
that every application or scheduler has no regression.

Descriptive paired log-ratio bootstrap 95% intervals for time are [-14.56%,
-1.23%] serial graph, [-23.69%, -12.06%] parallel graph and [-4.22%, +22.33%]
parallel linear. These intervals describe paired geometric means, while the
table reports median ratios. They are not release noninferiority guarantees.

## Initial campaign and self-control

The initial fixed 20-pair comparison covered all six existing binaries. It ran
under substantially different host timing conditions. Its median changes were:

| Operation | Time change |
| --- | ---: |
| serial graph | -20.47% |
| parallel graph | -0.46% |
| serial linear | -9.74% |
| parallel linear | +27.34% |
| replay | +5.15% |
| canonical DTO encoding | -3.65% |
| bounded outcome-log append | -1.09% |

Graph instructions still decrease 11.87% serial and 11.91% parallel; instructions
for the unchanged workloads differ by less than 0.01%. The unfavorable
parallel-linear wall result is retained, not discarded or attributed conclusively
to a specific cause. It has essentially unchanged instructions, cycles and
CPU time. Replay also retains its positive timing median.

A subsequent fixed 20-pair identical-source comparison (`99d9a3d` against
itself) gives +0.16% serial graph, +6.93% parallel graph and +0.07% parallel linear
median time changes. The unchanged serial graph median is about 504 ns, versus
785 ns in the initial comparison. The parallel control intervals include zero.
This demonstrates variable host conditions and motivated one fixed 40-pair
confirmation; there was no repeated selection of favorable samples. All three
campaigns and their raw observations are retained. No cross-campaign absolute
speed comparison or universal slowdown claim is made.

## Verification and evidence

- Two generated properties run 10,000 cases each on Rust 1.99 and 1.85: 40,000
  total comparisons against the frozen independent tree-based validator.
- Deterministic tests cover bounds, references, disconnected cycles, maximum
  branching and every one of the 384 bounded source/outcome keys. A duplicate
  with an unknown destination must still return the original destination error.
- Full workspace all-target/all-feature suites pass on stable and MSRV, excluding
  nonterminating fuzz executable targets. Strict workspace Clippy, warning-denied
  documentation, doctests, formatting, layer checks and graph update pass.
- `fuzz_linear_engine`, which also validates decoded graph definitions, completes
  100,000 iterations with seed 8. [All seven source CI jobs](https://github.com/STEXS-Technologies/penelope/actions/runs/41144694633)
  pass at `005ea60`.

[Evidence manifest](performance-data/2026-10-31-graph-keys/evidence.json) records
all 600 measured processes across the three campaigns. Deterministic archives
retain original JSON bytes and member SHA-256 hashes, alongside summaries,
compressed command logs, source CI, runner and statistics scripts. Extract a
campaign archive and pass its directory to `summarize-graph-key-comparison.py`
to reproduce its summary. The runner commands use `--runs 20` with all six
binaries initially, the three graph/control binaries for the self-control, then
`--runs 40` for the same three binaries with the candidate. All use `--perf`,
`--cpu 0` and `--parallel-cpus 0,1,2,3`.

Historical package archives, complete release fuzz campaigns and portable memory
bounds retain their original pinned revisions; this is scoped follow-up evidence.
