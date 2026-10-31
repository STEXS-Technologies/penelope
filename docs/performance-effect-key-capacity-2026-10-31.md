# Exact effect-key encoding capacity, 2026-10-31

`EffectKey::canonical_bytes` now reserves its complete output size once. The field order, identifier length prefixes, action tags, digests and attempt encoding are unchanged. Five two-byte identifier lengths, two 32-byte digests, a four-byte attempt and a one-byte kind require 79 fixed bytes plus the five identifier lengths. Saturating addition avoids unchecked arithmetic; validated identifier bounds keep actual output at most 719 bytes.

The final ordinary-allocator benchmark measures 34.88–82.12% less encoding time across all 40 profile/campaign comparisons. This measures construction and disposal of canonical effect-key bytes, not whole-workflow latency or application overhead. No public API, SQLite schema or durability setting changes.

## Measured results

Final executable, two campaigns; ranges cover four action kinds and both campaigns. Times are marginal medians; percentage improvements use paired geometric ratios and need not equal ratios of the displayed medians.

| Identifier profile | Baseline ns | Candidate ns | Less encoding time |
| --- | ---: | ---: | ---: |
| Short, 9 bytes each | 99.965–131.358 | 63.151–63.754 | 35.99–51.42% |
| Inline boundary, 22 bytes each | 92.862–105.213 | 60.428–60.928 | 34.88–41.18% |
| First heap size, 23 bytes each | 84.933–110.216 | 19.629–19.818 | 76.90–82.12% |
| Maximum, 128 bytes each | 83.827–103.933 | 21.562–22.097 | 73.36–79.28% |
| Mixed, 5/22/23/64/128 bytes | 136.856–157.934 | 33.432–33.638 | 75.05–79.23% |

The separate allocation executable finds one initial allocation in both versions. Baseline also requests four reallocations for uniform profiles or six for mixed lengths; candidate requests none. Total requested bytes change from 248 to 124 (short), 368 to 189 (inline boundary), 383 to 194 (first heap), 1,958 to 719 (maximum), and 1,016 to 321 (mixed). These are allocator requests, not resident memory.

The existing public SQLite completion probe independently confirms four fewer reallocations and 128 fewer requested bytes for each short-scope outbox acknowledgement or renewal. Acknowledgement changes from 6 to 2 reallocations and 1,828 to 1,700 bytes; renewal from 8 to 4 and 3,260 to 3,132 bytes. Initial allocation counts remain 10 and 11. Timer acknowledgement, scheduling and cancellation have identical counts and bytes. Each public case has 64 observations per variant. This warmed fixture measures idempotent acknowledgement retries and successive lease renewals, not first acknowledgements, all identifier sizes or latency.

## Method and evidence

Baseline is parent `98b8aacf3d00f1d964ba4422194b20403df54b8d`. Each experiment builds separate baseline/candidate release binaries with identical harnesses, changes only the capacity reservation, and verifies exact legacy bytes in 10,005 cases on both variants. All four kinds, varied identifier sizes, digest contents and attempt values are represented. Twenty candidate allocation profiles require exactly one allocation and exact output capacity; the old implementation fails that guard with five allocation requests on the first short-key case.

Each binary pair has two campaigns of six balanced ABBA/BAAB blocks, 48 ordinary executable invocations, 960 timing rows and 96,000,000 timed encodings. Each invocation measures all 20 profiles with 100 untimed warmups and 100,000 timed encodings per profile. Allocation counters run separately: 80 key observations and 640 public SQLite observations per experiment. Six paired block log ratios feed 20,000 deterministic bootstrap draws, with unadjusted per-comparison 95% intervals. All 40 intervals exclude zero in each experiment; there is no multiplicity-adjusted universal performance bound.

Both experiments are retained. The initial experiment measured 37.90–82.17% less encoding time. Clippy then required removing an unnecessary `Result` return from the ordinary executable's entry point, outside its timed loop. Both binaries were rebuilt and the complete experiment repeated using the final harness. The initial harness snapshot is archived and checked against its own hash. Across both experiments there are 192,000,000 timed encodings, 1,920 timing rows, 160 key allocation observations and 1,280 public allocation observations. No samples from the initial executable are pooled into the final table.

Timing ran on CPU 0 of a Ryzen 7950X, Linux 7.2.9, Rust 1.99.0, with no project builds, tests, graph work or allocator probes overlapping ordinary timing. Background load and CPU frequency were uncontrolled. This is a local microbenchmark, not a production workload forecast.

Stable all-feature workspace tests excluding the fuzz package and Rust 1.85 domain/SQLite tests pass. Final workspace all-target/all-feature strict Clippy, Rust 1.85 domain all-target checks, formatting and layer checks pass. The full tests cover the final production and test code; the later harness entry-point adjustment is covered by final all-target checks. Graph output is refreshed separately and remains outside the commit.

[Archived evidence](verification-data/2026-10-31-effect-key-capacity/) includes compressed raw samples, checksums, source and binary hashes, both analysis/build/measurement scripts, the production patch, negative allocation guard logs and qualification logs. Run `python3 docs/verification-data/2026-10-31-effect-key-capacity/verify.py` at this version to verify the manifest, current source hashes, baseline-to-candidate patch, legacy parity, guards, qualification and recomputed summaries. The verifier performs no source mutations. Release binaries are identified by hash rather than stored in Git.
