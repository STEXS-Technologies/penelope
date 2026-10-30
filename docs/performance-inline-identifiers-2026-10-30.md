# Inline identifier optimization and application overhead — 2026-10-30

A [later encoding and worker-polling follow-up](performance-encoding-2026-10-31.md)
reduces current-branch encoding time and fixes a queued-completion/exit race.
The initial identifier-stage results below remain pinned historical measurements.

The one-step in-memory engine's empty-task paired added-time median drops from
**245.29 ns to 119.17 ns**, a **51.42% reduction** versus the previous
optimized branch. For the same ~3.8 µs CPU task, measured overhead drops from
**6.34% to 2.87%**; for the ~61.6 µs task it drops from
**0.43% to 0.19%**.
These are workload-specific measurements of the in-memory engine, excluding
persistence, worker isolation and recovery services. They are not universal
Penelope overhead percentages or latency bounds.

## Change and compatibility

All 16 protocol identifier types now privately store short UTF-8 text inline and
long text in an owned `Box<str>`. On this 64-bit host, up to 22 bytes fit inline,
with the same 24-byte value size as the old `String`. Capacity follows the target's
`String` size to avoid fixing a 64-bit capacity on smaller pointer widths; only
stable/MSRV host layout/allocation checks are claimed here. The allocation-free
portable controller has no dependency on this hosted domain representation.

Borrowed short-identifier admission and short clones allocate zero times. Long
borrowed admission and long clones retain one allocation. Owned constructors may
trim spare capacity when converting a long `String` to `Box<str>`; their allocation
behavior is not asserted to match the old constructor. No shared reference counts,
interner, unsafe production code or new production dependency was introduced.

Canonical validation/prefixes and the 128-byte admission limit are unchanged.
Equality, ordering and hashing compare textual content across both storage forms.
Debug formatting retains `TypeName("text")`, display and Serde remain strings,
and v1 golden wire fixtures/canonical digest tests pass. Identifier value size,
short/long allocation counts and all public constructor routes are tested.

An exploratory `Arc<str>` representation reduced serial cost but slowed the
four-worker pilot, so it was replaced before the committed implementation. Inline
storage avoids that shared-reference-count contention. The initial inline stage
also showed slower encoding. The final source adds getter inlining; both stages'
fixed campaigns are retained, including the remaining encoding tradeoff.

## Identical application-task comparison

| CPU task | Direct median | With Penelope median | Paired added-time median | Before overhead | After overhead |
| --- | ---: | ---: | ---: | ---: | ---: |
| 64 kernel iterations | 44.70 ns | 154.70 ns | 110.18 ns | 560.53% | **249.54%** |
| 4,096 kernel iterations | 3838.94 ns | 3949.27 ns | 110.02 ns | 6.34% | **2.87%** |
| 65,536 kernel iterations | 61577.00 ns | 61698.53 ns | 114.64 ns | 0.43% | **0.19%** |

Each round computes `100 × (Penelope time / direct time − 1)` and added time
before taking the median over 160 pairs. These paired statistics need not equal
ratios/differences of the independently displayed marginal medians. The empty
task's ~1 ns direct path makes its percentage uninformative, so its absolute
added time is used as the engine-floor comparison instead. Raw summaries retain
all percentages.

The final ~61.6 µs short-identifier task's individual paired overheads range from
**−0.47% to +1.04%**. Host scheduling/frequency noise remains significant at this
scale; **the 0.19% median does not establish a worst-case ≤1% guarantee**.
The ~44.7 ns task still has 249.54% overhead. Smaller tasks and additional runtime
services change the percentage substantially.

The long-identifier control appends 100 ASCII bytes to every fixture identifier,
forcing heap fallback. Its empty-task added-time median changes from
**238.81 ns to 223.75 ns** (about 6% lower), much less than the short case.
Its ~3.8 µs task changes from **6.22% to 5.76%**, and its ~61.8 µs task
from **0.41% to 0.38%**. Identifier length materially affects the benefit.

## Existing reference workloads and tradeoff

All times are nanoseconds per reported operation, using unchanged benchmark
sources/datasets at both revisions. Parallel cases use four pinned CPUs and
include thread startup in the benchmark's elapsed time.

| Workload | Before median | After median | Time change |
| --- | ---: | ---: | ---: |
| Linear saga | 245.42 | 111.76 | -54.46% |
| Graph saga | 809.63 | 511.88 | -36.78% |
| Linear replay | 377.75 | 229.05 | -39.36% |
| Parallel linear saga | 84.96 | 32.29 | -62.00% |
| Parallel graph saga | 292.16 | 169.38 | -42.02% |
| Canonical DTO encoding | 331.20 | 352.04 | +6.29% |
| Bounded outcome append | 526.14 | 363.54 | -30.90% |

**Standalone canonical DTO encoding remains 6.29% slower** (about 21 ns per
encoding), despite the execution/replay/append improvements. Inline text needs
safe UTF-8 decoding when exposed as `&str`; getter inlining reduces some access
cost but does not remove it. The measured regression is retained, not waived as
noise. Applications dominated by serializing already-built values may see a
regression. No claim that every operation or every integration is faster is made.
Hardware counters cover the whole benchmark process; the contract benchmark's
counter totals combine encoding and append and cannot attribute instructions to
those two phases separately.

## Provenance and fixed method

- Baseline `46c0ab1`: previous production code (`4d9c292`) plus only the
  long-identifier benchmark option. Initial inline source `719cc07`; final
  measured source `aa2a557193771bbe9a5116b7ccaae967357f00ff` adds only three
  getter inlining attributes to the initial implementation.
- `69a749f` changes only a real-worker test's completion-wait budget. Measured
  production library sources are identical to `aa2a557`.
- Rust 1.99.0, bench profile, Ryzen 9 7950X, Linux; serial CPU 0, parallel CPUs
  0/1/2/3. Frequency, OS caches and unrelated host processes were uncontrolled.
  No local correctness builds, tests or fuzzing overlapped timed campaigns.
- Existing benchmark campaigns: fixed 20 alternating revision pairs per binary,
  one warmup process, Linux perf counters and GNU-time resource readings. The
  unchanged runner is `scripts/benchmark-decision-comparison.py`.
- Each application campaign: four process pairs for each short/long mode,
  40 alternating direct/managed rounds per work size, 100 warmup/check inputs
  per size. Identical CPU function, runtime seeds and checked checksums. Iteration
  counts are 100,000 / 100,000 / 2,000 / 128 for 0 / 64 / 4,096 / 65,536 work.
- Both initial and final stages retain 240 process records and 2,560 application
  records each. The second stage follows an actual source change; neither stage
  is discarded to select better timing. Source, lock and executable hashes and
  exact environment/commands are in metadata. Lock changes add only the existing
  workspace `proptest` as a domain dev dependency.

[Evidence directory](performance-data/2026-10-30-inline-identifiers/metadata.json)
links the compressed raw records, unrounded summaries, runners and validation
logs with a SHA-256 manifest. The application benchmark accepts
`--long-identifiers` on its executable for the heap-fallback control.

## Correctness and CI

Two new properties cover arbitrary UTF-8 storage equivalence and all 16 public
identifier types' admission/errors, constructor parity, ordering/equality/hashing,
debug/display and Serde bytes. Each runs 10,000 requested cases on stable 1.99.0
and MSRV 1.85.0, including a final source rerun. Explicit 0–128-byte and multibyte
boundary cases and an isolated allocation/value-size executable supplement them.

Stable and MSRV workspace all-target/all-feature suites, workspace doctests,
strict workspace Clippy, fmt and layer checks pass on the initial logic; final
getter/source checks and the seven-job code CI pass on `aa2a557`. Four relevant
local fuzz targets (newtypes, identifiers, DTOs and linear/graph engine) each pass
100,000 inputs at seed 5 on `719cc07`; final CI also covers the complete bounded
fuzz target set. This supplements the historical qualification campaigns rather
than claiming they were rerun in full on the new source.

A separate pre-existing CI failure on the preceding documentation commit
`4d9c292` exposed a real-worker test that exhausted 100 polls before completion.
Its test-only fix uses a bounded ten-second monotonic deadline, retaining strict
completion and authorization assertions. The test passes **31 repetitions on
stable and 31 on MSRV**, followed by the full real-runtime suite on each.
The failed log is retained. Runtime timeouts and production behavior are unchanged.

[All seven measured-source CI jobs](https://github.com/STEXS-Technologies/penelope/actions/runs/41134034437)
pass, including actual optional StateChronicle conformance, stable/MSRV,
portable core, supply chain, secrets and bounded fuzzing.

[All seven test-follow-up CI jobs](https://github.com/STEXS-Technologies/penelope/actions/runs/41134360757)
also pass on `69a749f`, whose production library sources match the measured source.
