# Outcome log scope comparison, 2026-10-31

`OutcomeLog::append` now compares all five scope fields by reference instead of constructing and discarding an owned scope for each outcome. Validation and error ordering remain unchanged, including validation before mutation. This removes four string clones per outcome without changing the public API or serialized format.

Across two campaigns, point estimates show 3.02–24.89% lower append time for the measured profiles. Thirty-one of 32 unadjusted per-profile 95% intervals exclude zero. The short-scope, long-outcome-ID streaming profile in campaign 2 is inconclusive (−3.017%, interval [−7.975%, +4.720%]); it must not be claimed as a proven improvement. These are append microbenchmarks, not application overhead percentages or SQLite throughput measurements.

## Method

The baseline is parent `a8f79b4c3ba178ba1865ea1165f9e644767afddb`; the candidate changes only the scope comparison in the measured runtime. Both use identical benchmark fixtures and ordinary allocators for timing. Two campaigns each contain six balanced ABBA/BAAB blocks, four processes per block: 48 ordinary timing processes and 768 rows. Each process measures all 16 combinations of scope length (9/128), outcome ID length (8/128), and append profile. Profiles are one outcome, one 128-outcome batch, 4,096 single-outcome appends, and one 4,096-outcome rebuild batch. Totals: 396,288 logs, 75,099,648 outcomes and 25,555,968 public append calls.

Scope/log construction, fixture creation, verification and log destruction are outside timing. Timed append calls include validation, storage clones, duplicate tracking and container growth. Every completed log is checked against the independently constructed fixture, including scope, full outcomes and next sequence. Fixture checksums must match both variants. This measures healthy appends; scope failure allocation behavior is checked separately.

CPU 0, Ryzen 7950X, Linux 7.2.9, release Rust 1.99.0. No project builds, tests, allocation probes or analysis overlapped ordinary timing. CPU frequency and unrelated background activity were uncontrolled. Each block compares the geometric mean of two candidate and two baseline runs; 20,000 deterministic six-block bootstrap samples produce the reported unadjusted intervals. Displayed marginal medians need not reproduce the paired ratio.

## Latency results

Times are nanoseconds per complete log. Negative change means less time.

| Campaign | Scope length | Outcome ID length | Profile | Baseline ns | Candidate ns | Paired change | 95% interval |
| --- | ---: | ---: | --- | ---: | ---: | ---: | --- |
| 1 | 9 | 8 | one | 112.481 | 102.233 | -13.168% | [-18.441%, -8.231%] |
| 1 | 9 | 8 | page | 8701.408 | 7419.935 | -15.219% | [-16.520%, -14.136%] |
| 1 | 9 | 8 | rebuild | 301262.859 | 247489.672 | -18.518% | [-19.433%, -17.528%] |
| 1 | 9 | 8 | stream | 533083.641 | 498069.484 | -6.721% | [-7.154%, -6.393%] |
| 1 | 9 | 128 | one | 149.854 | 139.328 | -8.788% | [-10.619%, -7.126%] |
| 1 | 9 | 128 | page | 14152.302 | 12428.675 | -12.569% | [-15.323%, -10.296%] |
| 1 | 9 | 128 | rebuild | 485174.078 | 432799.234 | -10.492% | [-11.067%, -9.930%] |
| 1 | 9 | 128 | stream | 861760.156 | 817437.359 | -4.408% | [-5.286%, -3.301%] |
| 1 | 128 | 8 | one | 165.871 | 128.353 | -22.787% | [-24.153%, -21.417%] |
| 1 | 128 | 8 | page | 15552.934 | 11744.976 | -24.891% | [-25.298%, -24.364%] |
| 1 | 128 | 8 | rebuild | 525999.859 | 402856.938 | -22.906% | [-23.331%, -22.395%] |
| 1 | 128 | 8 | stream | 777178.094 | 657850.688 | -15.169% | [-15.791%, -14.458%] |
| 1 | 128 | 128 | one | 213.993 | 169.582 | -18.670% | [-22.352%, -12.502%] |
| 1 | 128 | 128 | page | 21337.331 | 17436.243 | -16.755% | [-23.669%, -7.630%] |
| 1 | 128 | 128 | rebuild | 728958.484 | 601435.859 | -17.858% | [-18.463%, -17.214%] |
| 1 | 128 | 128 | stream | 1119883.656 | 1007517.828 | -10.639% | [-11.812%, -9.676%] |
| 2 | 9 | 8 | one | 113.442 | 102.927 | -9.871% | [-11.384%, -8.519%] |
| 2 | 9 | 8 | page | 8673.586 | 7321.858 | -14.980% | [-16.327%, -13.642%] |
| 2 | 9 | 8 | rebuild | 301703.125 | 247757.078 | -17.864% | [-19.336%, -16.220%] |
| 2 | 9 | 8 | stream | 529672.172 | 495889.203 | -5.948% | [-7.090%, -4.215%] |
| 2 | 9 | 128 | one | 149.166 | 139.428 | -8.182% | [-10.861%, -5.325%] |
| 2 | 9 | 128 | page | 13931.126 | 12382.332 | -11.043% | [-11.964%, -10.315%] |
| 2 | 9 | 128 | rebuild | 478352.406 | 431245.859 | -10.460% | [-11.885%, -9.118%] |
| 2 | 9 | 128 | stream | 856616.406 | 812309.172 | -3.017% | [-7.975%, +4.720%] |
| 2 | 128 | 8 | one | 162.621 | 129.171 | -21.030% | [-22.822%, -19.300%] |
| 2 | 128 | 8 | page | 15297.473 | 11655.788 | -24.452% | [-25.535%, -23.323%] |
| 2 | 128 | 8 | rebuild | 518698.453 | 401330.672 | -23.451% | [-24.729%, -22.353%] |
| 2 | 128 | 8 | stream | 769577.344 | 653487.422 | -15.143% | [-15.482%, -14.765%] |
| 2 | 128 | 128 | one | 212.394 | 169.659 | -21.180% | [-22.281%, -20.063%] |
| 2 | 128 | 128 | page | 21070.472 | 17270.200 | -17.847% | [-18.573%, -17.151%] |
| 2 | 128 | 128 | rebuild | 726418.391 | 600488.031 | -17.446% | [-18.871%, -15.988%] |
| 2 | 128 | 128 | stream | 1108561.297 | 1003653.781 | -9.730% | [-10.192%, -9.281%] |

## Allocation traffic

Separate allocation-counting executables produced 256 samples, eight per variant and profile. All eight counts within each profile agree. With maximum-width scopes, the candidate removes exactly four allocations and 512 cumulative requested bytes per accepted outcome, for either outcome ID length. One outcome saves 4 allocations/512 B; 128 save 512/65,536 B; 4,096 save 16,384/2,097,152 B in either streaming or rebuild mode. This is allocation traffic, not peak memory or RSS. Reallocation counts are unchanged. Short scopes use inline strings and show no allocation difference. Required accepted-outcome and duplicate-tracking allocations remain.

## Correctness and qualification

The integration guard exercises each of five scope coordinates, early and late failures, and scope lengths 5/22/23/128 (including the inline storage boundary): 40 cases. It checks scope-error precedence over sequence errors, unchanged log state on rejection, repaired append success, serialized reconstruction and duplicate detection. The isolated allocation guard rejects maximum-width mismatches with at most one allocation for the temporary ID set and then successfully appends the repaired fixtures for all four profiles. The baseline fails this guard with five allocations, establishing the regression test's sensitivity. Both measured runtime variants pass the focused semantic tests.

Full stable workspace all-target/all-feature tests excluding the fuzz package, Rust 1.85.0 all-target/all-feature ports and SQLite tests, strict workspace Clippy, formatting and layer checks passed. Graphify was updated after the code changes. Logs and hashes are archived with the raw measurements.

## Reproduction

Run `python3 docs/verification-data/2026-10-31-outcome-log-borrowed-scope/verify.py` from the repository root to check archived hashes and recompute summaries without changing production source. Source checks require this version of the checkout; later changes can legitimately invalidate them.

For a new measurement, use a fresh directory: run the archived `build.py target/outcome-log-repeat`, then `measure.py target/outcome-log-repeat`, then `analyze.py target/outcome-log-repeat`. Run from the repository root in an idle checkout. The build script temporarily substitutes the pinned baseline runtime and restores the original source in `finally`; no other builds, tests or source operations should overlap it. Measurement uses the copied binaries and refuses to overwrite existing raw evidence. Binaries are not archived; their original hashes are recorded.
