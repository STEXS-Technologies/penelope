# Encoding, identifier decoding and worker-exit follow-up — 2026-10-31

Against the previous branch, the fixed comparison measures **4.16% less time for
canonical DTO encoding**, **9.47% less for short-ID JSON decoding**, and **5.61%
less for escaped-ID decoding**. Long-ID decoding measures **4.03% more time**.
These component measurements do not establish a new application-wide overhead
percentage. The previous direct CPU-task overhead figures remain historical at
their pinned source revision.

## Implementation

Domain canonical encoding uses the same Serde JSON writer with per-type initial
buffer capacities, avoiding repeated 128 → 256 → 512 growth for common records.
Small scalars reserve less space. These are allocation hints, not bounds: long
identifiers, large admitted arrays and arbitrary serializable values still grow
normally. There is no second sizing pass, truncation or changed wire format.
The common benchmark outcome now uses one allocation. Some smaller records may
reserve unused capacity; no universal minimal-allocation/peak-memory claim is made.

Identifier deserialization admits borrowed Serde text directly through the existing
typed parser, avoiding a temporary owned String. Unescaped short-ID JSON decoding
allocates zero times for all 16 types. Owned strings and byte-string visitors
remain supported; invalid UTF-8, invalid types and noncanonical identifiers remain
rejected. Escaped JSON still needs the deserializer's scratch buffer. Long IDs
retain the existing heap fallback and do not get a zero-allocation guarantee.

## Fixed measured results

Nanoseconds per reported operation; 20 alternating revision pairs per benchmark,
one warmup process each, Rust 1.99.0 bench profile, Ryzen 9 7950X, Linux. Serial
CPU 0, four-worker cases CPUs 0/1/2/3. Sources/datasets and dependency locks match.

| Operation | Before median | After median | Time change |
| --- | ---: | ---: | ---: |
| canonical DTO encoding | 342.68 | 328.44 | -4.16% |
| identifier JSON decode short | 37.29 | 33.76 | -9.47% |
| identifier JSON decode escaped | 59.36 | 56.03 | -5.61% |
| identifier JSON decode long | 127.65 | 132.80 | +4.03% |
| bounded outcome-log append | 370.00 | 363.32 | -1.80% |
| linear-saga start/result/complete | 113.46 | 109.11 | -3.84% |
| graph-saga start/result/result | 496.47 | 503.56 | +1.43% |
| linear-saga replay | 224.56 | 226.13 | +0.70% |
| parallel linear-saga start/result/complete | 33.30 | 36.34 | +9.12% |
| parallel graph-saga | 158.95 | 156.55 | -1.51% |

The pure-engine controls were not algorithmically changed by this serialization
work. Their positive and negative timing shifts are retained. In particular,
parallel linear elapsed time is +9.12%, while its whole-process instructions are
+0.009%, task-clock −0.91% and cycles −0.46%. These readings do not isolate code
layout effects from scheduling/frequency variability and are not an all-workload
speed guarantee. The long-ID decode regression is also retained as a tradeoff.

Perf counters cover entire processes. Encoding and append share a process, as do
the three decode cases, so their instruction counts cannot be attributed to
individual phases. The encoding/append process uses approximately 7.72% fewer
instructions than the immediately preceding branch.

A separate 20-pair control compares with the pre-inline source `46c0ab1`. Its
encoding medians are 441.21 ns before and 333.71 ns after, but the old-source
encoding ranges from 323.81 to 569.40 ns and the optimized source from 319.34 to
613.11 ns. That wide spread makes the apparent 24.36% gain unsuitable as a precise
old-baseline speedup or ≤1% non-regression proof. Whole-process instructions are
within about 0.16% of the old source, including append. The earlier 6.29% encoding
regression has been reduced in the current-branch comparison; exact parity with
that older source is not established by these noisy timing samples.

No local builds, tests or fuzzing overlapped the timed campaigns. CPU frequency,
OS caches and unrelated host processes were uncontrolled. All 280 main process
records and 40 old-source control records are retained, with unrounded summaries,
resource/counter samples, executable/source hashes and exact environment metadata.
A separate long-string construction experiment did not improve its paired pilot
and was reverted; its patch and all 40 process samples are retained too.

## Worker polling correctness fix

The preceding documentation commit `94fd3cb` had one failed CI job: the sealed
worker/source-replacement test reported Faulted rather than Completed. Inspection
found that poll could read WouldBlock, then observe a child exit after the worker
had written its final frame. That path classified the exit as a fault before
consuming the queued valid observation.

`5262cb2` makes EOF a prerequisite for exit-based fault classification. Positive
reads, partial frames, WouldBlock and interrupted reads remain pending until the
observation can be validated or EOF confirms the pipe is drained. Valid complete
frames retain their existing precedence and validation; EOF without a complete
frame retains application/protection/CPU/crash classifications. Runtime limits,
fencing and independent effect verification are unchanged. No worker dispatch
latency improvement is claimed from this fix.

A deterministic unit regression prevents exit checks before EOF. Real workers
cover split completion frames and a truncated payload that must fault without
output. On each of stable and MSRV, 50 sealed-source replacement tests and 50
fragmented/truncated tests pass, along with the full workspace suites. The original
failed CI log is retained, not treated as a harmless test timing flake.

## Verification and provenance

- Performance baseline `2276030` adds only the decode benchmark to the
  EOF-corrected runtime and prior encoding logic. Final source
  `53734a1d1ceedb971a18c2b108558b6725fbe568` contains the allocation/visitor changes.
- Stable 1.99.0 and MSRV 1.85.0 workspace all-target/all-feature tests pass.
  Three properties each run 10,000 requested cases on both toolchains: arbitrary
  identifier storage semantics, all 16 public identifier contracts, and exact
  old-serializer byte equivalence across arbitrary allocation hints/UTF-8 values.
- Eight golden DTO fixtures now assert exact canonical bytes against
  `serde_json::to_vec`, including field order. The DTO fuzzer also checks that
  byte-for-byte equivalence for the affected domain values.
- Identifiers, DTOs and worker-protocol fuzz targets each pass 100,000 inputs,
  seed 6. These are additional scoped checks, not a rerun of all historical
  release-qualification campaigns.
- Strict workspace Clippy, doctests, fmt and layer checks pass; Graphify is updated.
- [Seven-job runtime-fix CI](https://github.com/STEXS-Technologies/penelope/actions/runs/41136619735)
  and [seven-job combined-source CI](https://github.com/STEXS-Technologies/penelope/actions/runs/41137052190)
  pass, including optional actual StateChronicle conformance and bounded fuzzing.

[Evidence and validation metadata](performance-data/2026-10-31-encoding/metadata.json)
and the adjacent SHA-256 manifest retain the full records/logs. The existing
`scripts/benchmark-decision-comparison.py` is unchanged. A retained wrapper only
registers `identifier_decode` as an additional benchmark; measurement/parsing
logic is shared with the existing runner.
