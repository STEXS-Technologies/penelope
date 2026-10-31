# SQLite task replay statement reuse — 2026-10-31

Code: `91cdfcfae6c3942914cd4161f83ab3ed4c7d9895`. Baseline: `47c4a259e8a8c2bc23d848e36ff4ffb797c219a0`.

Reconstruction now checks out its bounded record scan, scoped blob lookup, and scoped byte-sum statements from the connection's existing prepared-statement cache. It holds the blob lookup statement for the whole replay, rebinding its scope and digest for each distinct full BlobRef. Previously every changing reference prepared and finalized another copy of the same SQL. The transaction-local single-reference memo and canonical scope decoder remain intact.

Only compiled statements are reused. Rows are fetched from the caller's current transaction snapshot; each selected record is hashed, decoded, and applied, and each non-memoized blob is checked for type, size, length, schema and digest. Head and raw-byte quota validation remain. There is no cached row/result, new cross-replay verification memo, wire/schema/dependency change, or unbounded collection. Standalone blob reads continue to prepare their single query without a cache-key allocation and share the same verified-row helper. Oversized standalone blob requests are still rejected before SQL preparation.

## Method

Ryzen 9 7950X, Linux 7.2.9, Rust 1.99 release builds, CPU 0 affinity. Each primary phase has 24 AB/BA pairs, 480 sequential isolated processes, 19 latency statistics and 9,216 Rust allocation samples. Profiles cover 2/16/128 records, canonical/pretty JSON, and repeated/alternating checkpoints, plus ordinary and external-write-before-every-append commit profiles. Each retry observation contains 250 public already-reserved authorization calls; each commit observation contains 128 appends. Setup and warmups are outside timing. No project builds/tests, graph updates or bootstrap analyses ran concurrently with measurements.

Negative percentages mean faster. Statistics are means of paired candidate/baseline percentage changes, with deterministic 20,000-resample percentile bootstrap 95% intervals. Every observation is retained. Campaigns are separate, not pooled. Host/storage variability is substantial in several commit and pretty-history profiles; wide intervals are reported rather than removed. A percentile interval barely excluding zero is not strong practical evidence by itself. These measurements do not quantify global application slowdown.

## Final primary results

| Operation | History/blob bytes | Change | 95% interval |
|---|---:|---:|---:|
| commit, ordinary | 0 | +57.41342% | [-0.37256, +166.65052]% |
| commit, ordinary | 32 | +10.68086% | [-0.59759, +31.42302]% |
| commit, ordinary | 4096 | +3.74387% | [-0.80186, +9.42490]% |
| commit, ordinary | 1048576 | +2.55088% | [+0.00743, +6.40144]% |
| commit, forced replay | 0 | +19.91018% | [-1.67632, +60.59098]% |
| commit, forced replay | 32 | -2.68459% | [-5.34110, -0.54051]% |
| commit, forced replay | 4096 | -0.66586% | [-2.61317, +2.50077]% |
| retry, canonical, repeated | 2 | -17.56891% | [-18.39776, -16.90458]% |
| retry, canonical, repeated | 16 | -13.35487% | [-14.94982, -12.37763]% |
| retry, canonical, repeated | 128 | -4.23320% | [-8.93516, -1.63892]% |
| retry, canonical, alternating | 2 | -20.48236% | [-26.14467, -17.22328]% |
| retry, canonical, alternating | 16 | -37.84726% | [-39.49783, -36.81223]% |
| retry, canonical, alternating | 128 | -45.97004% | [-47.44187, -43.97653]% |
| retry, pretty, repeated | 2 | -14.44326% | [-18.26558, -10.18536]% |
| retry, pretty, repeated | 16 | -6.85987% | [-8.79760, -4.89345]% |
| retry, pretty, repeated | 128 | +6.32195% | [-1.71937, +19.32765]% |
| retry, pretty, alternating | 2 | -10.37301% | [-14.91628, -4.64307]% |
| retry, pretty, alternating | 16 | -19.49600% | [-25.57773, -9.35684]% |
| retry, pretty, alternating | 128 | -10.61680% | [-30.53752, +26.34267]% |

Canonical alternating history 128 improves 45.97004% (mean observed time 363.53 to 195.43 microseconds), history 16 improves 37.84726%, and history 2 improves 20.48236%. Canonical repeated histories improve 17.56891%, 13.35487%, and 4.23320% at histories 2, 16, and 128. These gains are relative to the already optimized scope-decoder baseline, not the original release.

Pretty repeated history 128 and pretty alternating history 128 remain inconclusive. The primary ordinary 1 MiB commit estimate regresses 2.55088% with a borderline positive interval; the independent confirmation below estimates -1.84186% with an interval crossing zero. No general commit speedup or absence of regression is claimed from these noisy data.

## Allocation tradeoff and rejected per-reference trial

The final replay has exactly **four Rust allocations, 376 requested bytes, zero reallocations** in every measured format/checkpoint/history profile, versus the baseline's one allocation, 96 bytes. That is a fixed extra three allocations/280 bytes per replay, independent of history length in the tested range. It reflects rusqlite cache-key handling; SQLite C allocations and total resident memory are not measured. Statement-cache capacity remains the existing connection bound.

The first trial checked out a cached blob statement for each verified reference. At alternating history 128 it made 130 Rust allocations/10,456 requested bytes per retry; the fixed-statement design reduces this to four/376. The per-reference trial's canonical alternating history-128 latency improved 38.44%, but repeated histories and commits were noisy and inconclusive. That trial was rejected as the final design. Its independent raw campaign, confirmation, source snapshots, hashes and qualification logs are preserved under `per-reference/`. Trial qualification used rustfmt-only source changes relative to its measured snapshot; only final code has exact measured-source identity verified.

## Independent three-variant commit confirmation

A fresh 24-block run covers all six permutations of historical pre-guard, current guarded baseline and final candidate four times, alternating profile order. It comprises 144 processes and 504 latency observations. Historical control `50856b001f81978daea5044947557a26429d9740` lacks necessary external-history validation and is used only for comparison. The unchanged fixture and prior executable identities are hash-verified; final production behavior retains the guard.

| Profile | Blob bytes | Final vs guarded baseline | Final vs pre-guard control |
|---|---:|---:|---:|
| ordinary | 0 | -5.16657% [-14.48303, +3.09709] | +2.10453% [-6.44058, +11.22709] |
| ordinary | 32 | +2.84144% [-2.93244, +10.00516] | +0.80829% [-7.70837, +10.58962] |
| ordinary | 4096 | +22.19559% [-2.76822, +69.81754] | +8.63592% [-7.86620, +32.78610] |
| ordinary | 1048576 | -1.84186% [-5.57286, +1.02704] | -2.35980% [-7.41681, +1.06798] |
| forced replay | 0 | -2.09349% [-6.51357, +2.61026] | +243.97175% [+217.00643, +262.37290] |
| forced replay | 32 | +9.69453% [-13.18057, +46.67543] | +272.70143% [+256.65891, +292.73688] |
| forced replay | 4096 | +8.46540% [-5.50143, +34.38443] | +250.48522% [+226.26635, +288.25532] |

All final/baseline confirmation intervals cross zero, for ordinary commits and forced replay alike. This experiment establishes useful replay-retry improvements, not a precise commit-latency improvement. The forced-replay final/control point estimates remain approximately +244% to +273%, with broad intervals; retaining validation still costs substantially more than the historical incomplete check. Ratios of arithmetic mean times and means of paired percentage changes differ under outliers, so tables identify the statistic explicitly.

Including the rejected phase and both confirmations, evidence contains 1,248 measured processes, 80 latency statistics and 18,432 allocation samples. Rejected and final results are not pooled.

## Reliability and qualification

A new public-API regression alternates two scopes with the same blob digest on one journal. Across eight rounds it warms both scopes, externally deletes the first scope's blob, checks blob-read and reconstruction rejection without changing durable rows, confirms the healthy scope still works, repairs the missing blob, creates/drops an index and checks successful rebinding and reconstruction. Both scopes then reopen successfully. This tests fresh rows, scope bindings and schema invalidation rather than comparing implementation details. A deliberate query fault that ignores the scope binding incorrectly serves the other scope's identical-digest blob; the new regression detects this as `Ok(5)` where `Corrupt` is required. Production source is restored and the regression passes again on stable and Rust 1.85.

Final stable workspace all-target/all-feature tests (excluding the separate fuzz crate), SQLite all-target tests on Rust 1.85, strict workspace Clippy, formatting, layer boundaries and graphify update passed. SQLite has 27 unit, 51 process and 39 task tests. Sixty additional isolated stable/MSRV processes repeat the new scope/schema test and the existing external-cache and replay-memo suites: 160 delete/repair/schema rounds, 320 public rejection checks in the new test, 960 external-cache damage fixtures, and 180 replay-memo fixtures with 720 rejection checks. Existing bounds, serialization, corruption and decoder tests are included in full qualification.

The prior pushed scope-decoder commit's GitHub Actions run `41176716770` completed successfully, including bounded fuzzing, MSRV, stable verification and optional StateChronicle conformance. Those CI results describe that prior commit; the final local checks above exclude the separate fuzz crate and do not claim a fresh optional StateChronicle run.

[Evidence and reproduction](verification-data/2026-10-31-task-replay-statements/README.md) includes raw data, source snapshots, executable hashes, per-phase logs, standard-library analysis scripts and a standalone verifier. The release plan in Downloads was not copied into the repository.
