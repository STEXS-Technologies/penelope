# Atomic commit effect validation — 2026-10-31

Comparing borrowed effect coordinates instead of constructing owned `EffectKey`
values reduced local valid-batch validation latency by about **41% with short
identifiers** and **75% with 128-byte identifiers**, at 128 actions. The long-ID
allocation probe fell from **41,288 allocations to 8 per validation**. These are
public `AtomicProcessCommit::validate` measurements, not overall Penelope overhead.
The accompanying short-ID SQLite campaign did **not** establish faster durable
commits: both commit-latency intervals crossed zero.

## Source and preserved contract

Baseline: `245da19ea8d8e7f11b50c7ef7f7d9983293830b7`.
Candidate: `bc0c973cc134933727db3a3e1af98292201d99e3`.

The original action loop built an owned effect key for each action and each
later action it compared. Each key clones five identifier values. Short IDs use
inline storage, so these clones do not allocate. Long IDs use boxed strings, so
the repeated clones allocate. A 128-action valid batch constructs 8,256 keys,
cloning 41,280 identifiers in this part of validation alone.

A private `same_effect_key` helper in `penelope-ports` compares all nine effect
coordinates by reference: tenant, process, definition ID/version/digest, step,
attempt, action kind, and payload digest. Action ID remains excluded from semantic
effect identity and separately checked for duplicates. Exhaustive `ProcessAction`
destructuring makes an added field require review. The helper adds no public API.

The ordered pairwise search remains bounded by the existing 128-action limit;
its asymptotic complexity is unchanged. Scope/validity checks, early-return order,
input/source/causation checks, and typed error precedence remain in place. There
is no new set allocation, sorting, schema migration, encoding, digest or durability
policy. `EffectKey` itself and its public construction/encoding remain unchanged.

## Correctness evidence

A test-only reference preserves the complete validator from the pinned baseline,
with only its receiver renamed into a function argument. The verifier checks
its body against the archived baseline source after whitespace normalization.
Ten directed cases change each semantic coordinate in turn and confirm that a
changed action ID alone still produces `DuplicateActionEffectKey`.

A deterministic seed-42 corpus compares complete validation results, including
exact error variants, across 10,000 mixed short/long cases. It includes batches
up to 128 actions, duplicate IDs and effects, individual coordinate changes,
invalid attempts, inconsistent scope/sequence, and input/source/causation changes,
including combinations of these faults. This is bounded differential coverage,
not an exhaustive proof of all possible invalid inputs.

The final corpus passed in 50 isolated test processes, 25 each on Rust 1.99.0
and 1.85.0: **500,000 generated cases plus 500 directed cases**. Each process
has a 30-second guard. Existing public contract tests retain batch-bound and
other validation coverage.

## Measurement

Rust 1.99.0 release binaries on AMD Ryzen 9 7950X. Every process is pinned to
one permitted CPU; SQLite workers inherit that affinity. Both campaigns use
20 alternating AB/BA pairs, with no discarded paired trials. No own builds,
tests or graph jobs ran during timing. The two campaigns compile the same
production change; they use separately archived fixture versions.

The short-ID campaign runs allocation, CPU and public SQLite history probes for
each variant: 120 processes total. The long-ID campaign runs allocation and CPU
probes only: 80 processes total. The CPU executables use the ordinary allocator;
allocation counters are confined to separate executables so they do not distort
CPU timings. The allocation span covers one validation after fixture construction
and one warmup validation; returned success is checked outside the count span.

CPU profiles contain one valid input, one outcome, and 0, 1, 16 or 128 unique
actions with distinct step identifiers. After 100 warmup calls, they time
100,000/100,000/10,000/1,000 calls respectively for short IDs, and one tenth
those counts for long IDs. `black_box` prevents eliminating requests/results.
Setup and printing are excluded. Short fixtures use identifiers fitting inline
storage; long fixtures pad every identifier to 128 ASCII bytes. The zero-action
control has no duplicate-effect comparisons and establishes no clear latency
change in either campaign.

The short-ID SQLite `claim_history` probe uses fresh local WAL/FULL stores,
1,024 retained actions populated in eight public commits of 128 actions each.
Outbox records are claimed and acknowledged; timers are scheduled for future
logical time and remain unclaimed. It times open, commits, scheduling, initial
outbox claims/acknowledgements, then 1,000 empty polls after 100 warmups. It checks
retained state, exact claim order and token counters. Timer outgoing actions
remain pending; this is a storage fixture, not a complete delivery workflow.
See [the history methodology](performance-sqlite-claim-latency-2026-10-31.md)
for the unchanged fixture's timing boundaries. No long-ID SQLite campaign was run.

Changes average 20 within-pair candidate/baseline ratios. They are not ratios
of the aggregate means. Intervals bootstrap those ratios 10,000 times with seed
42, using sorted positions 250 and 9,749. All 17 latency metrics are reported;
intervals are individual/descriptive, without multiplicity correction. Filesystem
synchronization and scheduling contribute substantial variation in SDK writes.
Measurements do not establish invalid-batch performance, other action/outcome
mixes, other hardware/storage, Rust 1.85.0 latency or total application overhead.

## CPU validation latency

Means below are nanoseconds per validation; negative change is faster.

| Identifiers | Actions | Baseline ns | Candidate ns | Mean paired change | 95% interval |
| --- | ---: | ---: | ---: | ---: | ---: |
| Short | 0 | 29.459 | 29.764 | +1.0821% | [-0.5143%, +2.9790%] |
| Short | 1 | 46.384 | 36.279 | -21.7593% | [-22.3775%, -21.2397%] |
| Short | 16 | 2666.470 | 1576.381 | -40.8518% | [-41.5294%, -40.3289%] |
| Short | 128 | 167481.940 | 97923.487 | -41.5279% | [-42.2945%, -40.6457%] |
| 128-byte | 0 | 93.466 | 91.851 | -1.0455% | [-4.5240%, +1.5689%] |
| 128-byte | 1 | 151.402 | 101.277 | -32.6897% | [-35.1359%, -31.0222%] |
| 128-byte | 16 | 8015.769 | 1906.519 | -76.0890% | [-76.9176%, -75.4710%] |
| 128-byte | 128 | 447219.580 | 112336.766 | -74.8496% | [-75.4387%, -74.2095%] |

## Allocation counts

All 20 pairs in each campaign reported identical allocation counts. Short-ID validation
reported zero allocations/reallocations/deallocations/requested bytes in both
variants for all four profiles. Long-ID counts follow; reallocations were zero,
and every counted allocation was deallocated within the validation span.

| Actions | Baseline allocations | Candidate allocations | Baseline requested bytes | Candidate requested bytes |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 8 | 8 | 1,024 | 1,024 |
| 1 | 13 | 8 | 1,664 | 1,024 |
| 16 | 688 | 8 | 88,064 | 1,024 |
| 128 | 41,288 | 8 | 5,284,864 | 1,024 |

The remaining eight allocations come from unchanged scope construction/comparison.
Requested bytes are **cumulative allocation traffic**, not peak live memory or
resident memory. The old validator does not keep all cloned keys alive together.

## Public SQLite latency, short IDs

Means are microseconds per API call. `false` is outbox; `true` is future timers.
`commit` is a batch of 128 actions, not one action; `idle` is an empty poll with
retained rows. `seed_claim` precedes the initial outbox acknowledgements.

| Metric | Baseline µs | Candidate µs | Mean paired change | 95% interval |
| --- | ---: | ---: | ---: | ---: |
| `history/false/ack` | 900.661 | 866.144 | -2.5280% | [-8.9789%, +4.7479%] |
| `history/false/commit` | 2623.749 | 2462.413 | -4.9983% | [-14.1645%, +7.9003%] |
| `history/false/idle` | 15.602 | 15.645 | +0.2839% | [-0.3188%, +0.8793%] |
| `history/false/open` | 5602.219 | 6097.428 | +17.7286% | [-2.1980%, +38.6040%] |
| `history/false/seed_claim` | 2285.946 | 2013.734 | -3.7145% | [-14.6891%, +6.3614%] |
| `history/true/commit` | 2786.878 | 2760.375 | +0.1079% | [-6.8193%, +7.4265%] |
| `history/true/idle` | 16.293 | 16.170 | -0.7519% | [-1.2285%, -0.2949%] |
| `history/true/open` | 7052.279 | 6890.289 | +3.1384% | [-11.1721%, +18.0647%] |
| `history/true/schedule` | 918.959 | 950.112 | +6.8036% | [-4.0269%, +18.1067%] |

Neither durable commit interval excludes zero. The small timer-idle difference
also appears in a path that does not call this validator; it should not be
attributed to eliminating effect-key clones. This campaign does not establish
that validation gains offset the indexes' previously measured write cost.

## Qualification and artifacts

With compiler wrappers cleared, candidate production code passed full workspace
all-target/all-feature suites excluding `penelope-fuzz`, using
`RUST_TEST_THREADS=1`, on Rust 1.99.0 and 1.85.0. Each compiler passed 23 SQLite
unit, 40 process integration and 26 task integration tests. After extending
nonproduction fixtures to cover long IDs and separating timing/counting probes,
the final ports all-target/all-feature suite passed on both compilers, including
38 unit tests and the mixed differential corpus. Workspace Clippy with
`-D warnings`, formatting, layer checks and `graphify update .` also passed.

Initial fixture compile failures (receiver renaming and an input-kind variant)
and a Clippy request to make the test PRNG `const` were fixed and their logs
retained. Full-workspace debug benchmark output is functional qualification,
not part of the controlled release timing campaigns. Baseline CI run
`41164048579` passed; candidate CI is tracked separately after push.

[Evidence directory](verification-data/2026-10-31-atomic-effect-validation)
contains exact compiled production/fixture snapshots, hashes, all 200 measurement
process outputs, compiler/CPU metadata, all summaries and scripts, 50 isolated
parity outputs, validation logs, and checksums. Executables are identified by
hash and are not stored in Git. Build metadata records the parent HEAD during
the uncommitted experiment; source hashes and `source-pins.json` identify the
candidate. Build scripts retain local paths and temporarily swap/restore the
ports implementation: use an isolated checkout and the corresponding archived
fixtures. Set `PENELOPE_BENCH_LONG_IDS=1` for the extended long-ID probes.

```sh
python3 docs/verification-data/2026-10-31-atomic-effect-validation/verify.py
```

The verifier checks complete pairs, source/fixture/binary pins, exact allocation
counts, all recomputed means/intervals, the baseline oracle, repetition coverage
and suite counts.
