# Effect-key comparison order — 2026-10-31

Checking varying effect coordinates before the shared process scope reduces
valid-batch validation work while retaining the complete effect identity check.
Across four 128-action profiles, mean paired validation latency fell 63.03–68.99%.
An independent SDK-only confirmation measured **9.8893% lower latency for outbox
commits of 128 actions** (95% change interval [−17.0582%, −2.4288%]); timer commit
results remained inconclusive.
This comparison is against the already optimized borrowed-key validator, not
the earlier implementation that cloned owned keys. Allocation counts remain
unchanged. These figures describe local public validator and SQLite fixtures,
not a total application-overhead percentage.

## Source and behavior

Baseline: `d6a4072dd06b2f4f4b453f63b990fc022a79d343`.
Candidate: `ae8279e66093809ae722a12d2448449c70c570d3`.

`same_effect_key` previously compared tenant, process, definition ID/version/digest,
then step, attempt, kind and payload. Valid actions in one atomic batch share
that scope, so the first five comparisons could not distinguish their effects.
The helper now checks step, attempt, kind and payload before those five scope
coordinates. It still compares all nine fields when they match. The comparisons
use built-in/validated domain equality and perform no application callbacks.

Later actions in the lookahead have not yet passed their own scope checks.
Their scope comparisons therefore remain essential: the change does not infer
that future actions are valid or drop shared fields from effect identity.
Duplicate action-ID checks, pair iteration, outer validation order, typed errors,
128-action bound and asymptotic complexity are unchanged. There is no new public
API, allocation strategy, schema, encoding, digest or durability-policy change.
The evidence verifier confirms the only production-file difference is this
helper's body.

## Correctness

The existing reference validator is pinned to `245da19`, before either effect-key
optimization. Complete result comparisons retain the exact typed error, not just
success/failure. The deterministic corpus contains 10,000 mixed short/long cases,
including duplicate action IDs/effects, individual semantic coordinate changes,
invalid attempts, scope/sequence and input/source/causation faults. Ten existing
directed cases distinguish every semantic coordinate while excluding action ID
from effect identity.

A new directed test uses 128 actions sharing one step with distinct payloads.
It verifies acceptance, rejection of one exact duplicate effect, and all five
invalid future-scope coordinates after creating that would-be duplicate. These
cases must produce `ActionScopeMismatch` rather than incorrectly treating the
future action as a same-scope duplicate. The seven cases run with short and
128-byte identifiers, adding 14 directed cases per test process.

Both tests passed in 50 isolated processes, 25 on each supported compiler:
**500,000 generated cases plus 1,200 directed cases**. Each subprocess has a
30-second guard. This is bounded regression coverage, not an exhaustive proof
of every possible request.

## Paired measurements

Rust 1.99.0 release binaries, bundled SQLite for the SDK fixture, AMD Ryzen 9
7950X. All processes are pinned to one permitted CPU; SQLite workers inherit
affinity. Twenty alternating AB/BA pairs run four profiles per variant: short
IDs with distinct steps, short IDs sharing a step and varying payload, then the
two corresponding 128-byte-ID profiles. Allocation and CPU probes run separately
for each profile, followed by the public SQLite history probe once per variant.
That is **360 successful processes**, with no discarded paired trials. No own
builds, tests or graph jobs ran during timing.

CPU fixtures contain one input, one outcome and 0, 1, 16 or 128 valid unique
actions. The step profile varies step IDs and keeps payload constant. The payload
profile shares the first step ID (retaining its identifier length), varies the
32-byte payload digest, and keeps attempt/kind constant. Every profile must pass
public validation. After 100 warmups, short-ID fixtures time
100,000/100,000/10,000/1,000 calls; long-ID fixtures time one tenth those counts.
Setup, payload variation and printing are excluded; `black_box` prevents
eliminating calls/results. CPU binaries use the ordinary allocator.

The separate allocator executables count one validation after fixture setup and
one warmup. All 20 pairs reported unchanged counts in both variants: short IDs
allocate zero times; 128-byte IDs allocate/deallocate eight times and request
1,024 cumulative bytes. Reallocations are zero. These are allocation traffic
counts, not a peak-memory measurement. This change adds no further allocation
reduction to the preceding borrowed-key optimization.

Changes average the 20 within-pair candidate/baseline ratios, rather than ratios
of aggregate means. Bootstrap intervals use 10,000 resamples, seed 42 and sorted
positions 250 and 9,749. All 25 primary latency metrics follow; the independent confirmation reports nine more. Intervals are individual
and descriptive, without multiplicity correction. Neither the zero-action nor
one-action CPU profile invokes `same_effect_key`; changes in those controls are
not claimed as effects of reordering it. Results do not establish gains for all
request shapes, invalid batches, other machines, Rust 1.85.0 latency or complete
application workflows. Do not multiply these percentages with earlier campaigns
to claim a measured cumulative improvement.

## Validator CPU results

Means are nanoseconds per validation. Negative change is faster.

| Profile | Actions | Baseline ns | Candidate ns | Mean paired change | 95% interval |
| --- | ---: | ---: | ---: | ---: | ---: |
| `short-step` | 0 | 29.741 | 30.009 | +0.9456% | [-1.1559%, +3.2991%] |
| `short-step` | 1 | 37.649 | 37.456 | -0.2261% | [-2.7450%, +1.6731%] |
| `short-step` | 16 | 1627.032 | 708.354 | -56.4277% | [-57.2768%, -55.6799%] |
| `short-step` | 128 | 100259.108 | 36966.008 | -63.0316% | [-63.9369%, -62.3165%] |
| `short-payload` | 0 | 32.540 | 29.831 | -5.6373% | [-12.2217%, -0.5928%] |
| `short-payload` | 1 | 40.465 | 37.273 | -4.1355% | [-11.6471%, +1.3707%] |
| `short-payload` | 16 | 1636.866 | 703.484 | -56.8844% | [-58.0948%, -55.9525%] |
| `short-payload` | 128 | 100907.008 | 36902.526 | -63.4023% | [-63.9401%, -62.9021%] |
| `long-step` | 0 | 89.412 | 90.052 | +0.7710% | [-0.7122%, +2.5871%] |
| `long-step` | 1 | 99.010 | 99.979 | +1.0191% | [-0.5213%, +2.5749%] |
| `long-step` | 16 | 1838.811 | 742.001 | -59.6393% | [-60.0817%, -59.1747%] |
| `long-step` | 128 | 111953.269 | 34709.145 | -68.9930% | [-69.4117%, -68.5091%] |
| `long-payload` | 0 | 89.580 | 90.705 | +1.3064% | [-1.0093%, +4.1208%] |
| `long-payload` | 1 | 100.026 | 100.859 | +0.9292% | [-1.2843%, +3.3534%] |
| `long-payload` | 16 | 1930.144 | 827.064 | -57.1333% | [-57.9868%, -56.0397%] |
| `long-payload` | 128 | 117889.443 | 41923.536 | -64.3993% | [-65.0269%, -63.8790%] |

## Public SQLite results

The unchanged short-ID `claim_history` fixture uses local fresh WAL/FULL stores,
1,024 retained records and eight commits of 128 actions. Outbox actions are
claimed and acknowledged; timers are scheduled in the future and remain unclaimed,
with their outgoing actions still pending. It times setup APIs separately, then
1,000 empty public polls after 100 warmups, and checks retained state/order and
exact token counters. This is storage qualification, not a complete delivery
workflow. See [the full history methodology](performance-sqlite-claim-latency-2026-10-31.md)
for unchanged timing boundaries. No long-ID SQLite workload was timed here.

Means below are microseconds per API call. `false` is outbox, `true` is future
timers; a commit call contains 128 actions. Filesystem synchronization and
scheduling contribute substantial write variation. Of these paths, the atomic
commits invoke the modified validator; gains seen in unrelated paths should not
be attributed to the reordered effect comparison.

| Metric | Baseline µs | Candidate µs | Mean paired change | 95% interval |
| --- | ---: | ---: | ---: | ---: |
| `history/false/ack` | 1214.272 | 1285.186 | +19.6838% | [-1.0919%, +43.2251%] |
| `history/false/commit` | 3561.074 | 2168.555 | -13.7362% | [-24.8856%, -4.1480%] |
| `history/false/idle` | 15.672 | 15.502 | -1.0203% | [-2.2017%, +0.0162%] |
| `history/false/open` | 35088.091 | 12850.699 | +144.8574% | [-9.9807%, +434.7763%] |
| `history/false/seed_claim` | 2326.392 | 2300.951 | +4.2130% | [-10.8289%, +22.0329%] |
| `history/true/commit` | 2700.180 | 2561.243 | -4.9279% | [-10.6284%, +0.9587%] |
| `history/true/idle` | 16.535 | 16.341 | -0.7587% | [-4.0454%, +1.9863%] |
| `history/true/open` | 6638.035 | 7194.240 | +16.4112% | [-4.8676%, +39.1969%] |
| `history/true/schedule` | 1184.911 | 1042.333 | -4.5154% | [-16.0671%, +7.5591%] |

Outbox commit latency decreased in this fixture.
Timer commit latency did not establish a change (the interval crosses zero).

## Independent SQLite-only confirmation

The primary campaign contained large opening-time outliers in an unaffected
path. Its outbox-open mean paired change was +144.86%, with a wide interval
[−9.98%, +434.78%]. We therefore retained the complete campaign and ran one
predeclared separate SDK-only confirmation, using the same release binaries
and another 20 alternating pairs (40 processes). No own builds, tests or graph
jobs ran during it. All nine confirmation metrics are reported separately;
they are not pooled or selected in place of the primary campaign.

| Metric | Baseline µs | Candidate µs | Mean paired change | 95% interval |
| --- | ---: | ---: | ---: | ---: |
| `history/false/ack` | 965.455 | 951.925 | +2.4699% | [-8.7891%, +13.5334%] |
| `history/false/commit` | 2566.673 | 2257.580 | -9.8893% | [-17.0582%, -2.4288%] |
| `history/false/idle` | 15.417 | 15.315 | -0.6514% | [-1.2710%, -0.0298%] |
| `history/false/open` | 5941.494 | 6061.569 | +10.3387% | [-8.7785%, +31.7120%] |
| `history/false/seed_claim` | 2154.404 | 2127.058 | +1.8838% | [-9.0338%, +13.3927%] |
| `history/true/commit` | 2641.818 | 2496.125 | -4.4291% | [-12.5437%, +3.8975%] |
| `history/true/idle` | 16.053 | 16.048 | -0.0252% | [-0.6588%, +0.6026%] |
| `history/true/open` | 6764.273 | 6135.893 | +3.7982% | [-15.7485%, +24.9346%] |
| `history/true/schedule` | 979.783 | 982.916 | +4.5940% | [-6.7675%, +15.8072%] |

Outbox commit latency in the confirmation decreased in this fixture.
Timer commit latency in the confirmation did not establish a change (the interval crosses zero).

These remain workload-specific storage results, not a general Penelope slowdown percentage.

## Qualification and retained evidence

With compiler wrappers cleared, candidate production code passed full workspace
all-target/all-feature suites excluding `penelope-fuzz`, using
`RUST_TEST_THREADS=1`, on Rust 1.99.0 and 1.85.0. Each compiler passed 23 SQLite
unit, 40 process integration and 26 task integration tests. Final ports suites
also passed on both compilers after the shared-step fixture retained the first
identifier's length, including 38 unit and both differential tests. Workspace
Clippy with `-D warnings`, formatting, layer checks and `graphify update .` passed.
Debug-suite benchmark output is functional qualification, not controlled timing.
Baseline CI run `41164911209` passed; candidate CI is tracked separately after push.

[Evidence directory](verification-data/2026-10-31-effect-comparison-order)
retains exact baseline/candidate and fixture sources, source diff, original
reference validator/source pin, build and qualification logs, compiler/CPU
metadata, all 400 measurement process outputs and binary hashes, all summaries/scripts,
50 isolated parity outputs, and checksums. Executables are not committed.
Build metadata records the parent HEAD during the uncommitted experiment;
compiled-source hashes and `source-pins.json` identify the final code commit.
Scripts retain original local paths and temporarily swap/restore the ports
implementation; reproduce them only in an isolated checkout with the archived
fixtures. `PENELOPE_BENCH_LONG_IDS=1` selects long IDs and
`PENELOPE_BENCH_VARY_PAYLOAD=1` selects the shared-step/payload profile.

```sh
python3 docs/verification-data/2026-10-31-effect-comparison-order/verify.py
```

The verifier checks the exact production change, all checksums/source/fixture/
binary pins, complete paired runs, all 34 recomputed metrics and intervals,
unchanged allocation counts, the original oracle, parity runs and suite counts.
