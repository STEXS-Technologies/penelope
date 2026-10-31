# Atomic outcome scope validation audit — 2026-10-31

The existing `AtomicProcessCommit::validate` implementation is retained. Four attempts to remove its owned outcome scope improved some cases but caused reproducible slowdowns in other cases. This audit adds coverage, allocation budgets, and a multi-outcome benchmark profile; it makes no production performance improvement claim.

Baseline: `2c9fb0c1d39dfb0d2e9ea8e0d585acb5b33f214a`.

## What remains protected

The new multi-outcome integration test checks 1,536 directed cases: identifier lengths 5, 22, 23, and 128; first, second, and last positions in a 128-outcome commit; all 32 masks of the five scope coordinates; and four competing-error conditions. It independently asserts the expected typed error and compares with the existing pinned legacy validator. Valid rows deliberately vary their event fields. These cases extend the existing seeded 10,000-case validator corpus.

A negative control disabling outcome scope comparison fails at `length=5 rank=0 mask=1 competing=0`. The production source is restored afterward. This demonstrates that the added test detects a missing scope check, including its required error precedence.

A standalone allocation guard covers 16 single-outcome profiles: short/128-character identifiers, 0/1/16/128 actions, and shared/varied payloads. Short profiles allow zero allocations; long profiles allow at most four allocations and 512 requested bytes, with no reallocations. These are upper bounds on validation alone and allow future improvements. Fixture construction is outside instrumentation. This guard does not measure latency, live memory, or whole-application overhead.

## Rejected implementations

All four candidates passed focused semantic tests. All were rejected on latency evidence. Each was compared against the same parent, with the same updated harnesses.

| Candidate | Change | Short IDs, 128 outcomes: paired changes across both campaigns | Supported regressions in those 16 summaries |
| --- | --- | ---: | ---: |
| borrowed-all | Borrow five scope fields for every outcome | +0.14% to +7.65% | 12 |
| outlined-scope | Outline borrowed comparison and skip reflexive first-row comparison | −1.42% to +1.65% | 8 |
| action-stage | Also outline the existing action-validation stage | +5.29% to +18.36% | 16 |
| single-specialization | Separate single-outcome and multi-outcome monomorphizations | +9.08% to +27.72% | 16 |

“Supported” means the unadjusted 95% paired-block bootstrap interval lies above zero. The first three candidates removed four allocations/512 requested bytes for long scopes in both row-count profiles. The fourth removed them only for single-outcome commits. That benefit did not justify the measured regressions. These results show that eliminating allocations alone is insufficient evidence for retaining an optimization. Compiler code generation is a possible explanation for the tradeoffs, not an independently established cause.

## Method and limits

The ordinary benchmark now accepts `PENELOPE_BENCH_MANY_OUTCOMES` to construct 128 valid outcomes; absence preserves one outcome. Both ordinary and allocation executables checksum the fixture before measurement. The ordinary executable uses the system allocator without counters. The separate allocation executable measures one validation per fixture. The existing long-ID and varied-payload flags remain presence-based.

Each candidate has two campaigns, six paired blocks per profile, alternating ABBA/BAAB order, and all 32 combinations of identifier width, outcome count, payload variation, and action count. Each ordinary invocation reports four action counts. There are 384 ordinary invocations and 32 allocation invocations per candidate. All observations are retained, including regressions. The analysis pairs geometric means within blocks and uses 20,000 deterministic bootstrap resamples of six blocks. Confidence intervals are not adjusted for multiple comparisons.

Across four candidates: 6,144 timing rows, 512 allocation observations, 256 latency summaries, 128 allocation comparisons, and 90,017,664 timed validations. Warmups are excluded from that count. The two campaigns reuse each compiled binary pair rather than independently rebuilding it.

Measurements ran on an AMD Ryzen 9 7950X, Linux, Rust 1.99.0, pinned to CPU 0, without concurrent project builds or tests during timing. Background activity and CPU frequency were not controlled. Maximum-width/multi-outcome/action-heavy profiles use fewer iterations to keep the comparison practical. These are validator microbenchmarks; they establish neither a universal Penelope slowdown nor an application-level overhead percentage.

## Evidence and verification

[Evidence archive](verification-data/2026-10-31-atomic-outcome-scope-audit/) includes compressed raw observations, exact candidate patches, build/test logs, source and binary hashes, host metadata, analysis scripts, and final qualification logs. No binaries or external release plans are included.

Run the read-only verifier from this exact checkout:

```sh
python3 docs/verification-data/2026-10-31-atomic-outcome-scope-audit/verify.py
```

It checks the archive manifest and current source hashes, reconstructs each rejected candidate from its pinned parent in a temporary directory, and recomputes every summary from raw observations. Historical `build.py` scripts are provenance for the experiment: they expect the candidate source installed and the original strict zero-allocation guard, which intentionally failed against baseline. The retained guard instead enforces the measured baseline allocation budgets. Do not run historical build scripts directly against the final checkout.

Final qualification covers stable workspace tests, Rust 1.85 ports/SQLite tests, workspace strict Clippy across all targets/features, formatting, and layer boundaries. Exact commands and exit statuses are recorded in `qualification.json`; the graph is refreshed separately with `graphify update .`.
