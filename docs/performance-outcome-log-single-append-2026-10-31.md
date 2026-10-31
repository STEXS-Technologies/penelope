# Single-outcome append specialization, 2026-10-31

`OutcomeLog::append` now avoids constructing a temporary duplicate-ID set for a single-outcome batch. It retains validation, exact scope matching, sequence and persistent duplicate checks before mutation. Multi-outcome batches retain the original loop and temporary set for internal duplicate detection. The single-item implementation is outlined to keep it out of the bulk loop's generated body; it reuses the existing domain scope validator. The shared replay bound is checked before either path. A valid single append cannot overflow its next sequence because the log is bounded to 4,096 outcomes.

In two measured campaigns, single-outcome append time is 12.17–34.52% lower, and streaming 4,096 single-outcome appends takes 16.68–24.99% less time. All 16 single-item/streaming per-profile 95% intervals exclude zero. This removes one allocation and 116 cumulative requested bytes per single append on the measured host: 4,096 allocations and 475,136 B for a 4,096-outcome stream. Reallocation counts are unchanged. A warmed short-ID append with spare persistent-container capacity allocates zero times; constructing a fresh log's persistent containers and retaining long identifiers still require allocations.

These are append microbenchmarks, not a new overall application overhead percentage. Bulk performance is not proven identical: 13 of its 16 intervals include zero; one comparison shows an improvement, and two show isolated regressions. In campaign 2, maximum-width scope/short-ID rebuild is +0.863% [ +0.268%, +1.432% ], and maximum-width scope/long-ID page append is +3.747% [ +0.276%, +10.374% ]. Neither regression repeats with statistical support in campaign 1; the latter profile instead improves there. The complete observations, including these results, are retained.

## Method and rejected candidates

Pinned baseline: `97579da3fb3d304e7317ea78250a8fab8784c7e3`. Both variants use identical existing fixtures and ordinary allocators for latency. Each candidate was compared with that same baseline in two campaigns, each with six balanced ABBA/BAAB four-process blocks. Per candidate: 48 ordinary processes, 768 timing rows, 396,288 completed logs, 75,099,648 outcomes and 25,555,968 append calls. Separate counting executables provide 256 allocation samples per candidate. Fixtures cover scope lengths 9/128, outcome ID lengths 8/128, and four profiles: single outcome, one 128-outcome page, 4,096 single-outcome appends, one 4,096-outcome rebuild batch.

The first candidate used an optional temporary set in the validation loop; the second shared an inline validation helper between single and bulk paths. Both saved allocations but produced repeated bulk regressions, so neither is retained. The final candidate preserves the original bulk loop and calls a separate method for single outcomes. All three candidates' raw observations, patches, source/binary hashes, allocation negative controls, focused semantic tests and summaries are archived. They are successive experiments, not independent confirmation of one final implementation.

| Candidate | Single-outcome change range | Streaming change range | Bulk change range | Bulk profiles with positive lower 95% bound in both campaigns |
| --- | ---: | ---: | ---: | ---: |
| Rejected optional set | -36.977% to -18.614% | -30.531% to -17.880% | +0.271% to +3.269% | 3 |
| Rejected shared inline validation | -36.780% to -12.080% | -29.116% to -16.307% | -1.420% to +4.238% | 3 |
| Retained outlined single append | -34.522% to -12.174% | -24.993% to -16.679% | -2.229% to +3.747% | 0 |

Timings include public append validation, accepted-outcome cloning, ID tracking and container growth. Fixture/scope/log construction, verification and destruction are outside timing. Every completed log must match its independent fixture's scope, full outcome history and next sequence; fixture checksums agree across variants. Only healthy appends are timed. Allocation probes run separately from latency.

Host: CPU 0, Ryzen 7950X, Linux 7.2.9, release Rust 1.99.0. No project builds, tests, allocation probes or analysis overlapped ordinary timing. CPU frequency and unrelated background load were uncontrolled. Each block compares geometric means of its two candidate and two baseline processes. Twenty thousand deterministic block-bootstrap samples produce unadjusted per-profile 95% intervals. Displayed marginal median times need not reproduce paired percentages. No multiple-comparison adjustment or universal non-regression bound is claimed.

## Retained candidate latency

Times are nanoseconds per complete log; negative change means less time.

| Campaign | Scope length | Outcome ID length | Profile | Baseline ns | Candidate ns | Paired change | 95% interval |
| --- | ---: | ---: | --- | ---: | ---: | ---: | --- |
| 1 | 9 | 8 | one | 102.306 | 76.332 | -25.608% | [-26.495%, -24.628%] |
| 1 | 9 | 8 | page | 7307.185 | 7289.691 | -0.549% | [-1.535%, +0.446%] |
| 1 | 9 | 8 | rebuild | 246863.375 | 246860.922 | +0.574% | [-1.393%, +2.899%] |
| 1 | 9 | 8 | stream | 495930.141 | 372074.812 | -24.919% | [-25.330%, -24.618%] |
| 1 | 9 | 128 | one | 137.038 | 90.004 | -32.656% | [-34.441%, -29.848%] |
| 1 | 9 | 128 | page | 12355.566 | 12378.572 | +0.607% | [-0.240%, +1.785%] |
| 1 | 9 | 128 | rebuild | 428340.953 | 431529.000 | +0.266% | [-0.266%, +0.614%] |
| 1 | 9 | 128 | stream | 810801.578 | 642543.453 | -20.617% | [-20.759%, -20.435%] |
| 1 | 128 | 8 | one | 129.999 | 109.806 | -16.439% | [-17.945%, -14.888%] |
| 1 | 128 | 8 | page | 11645.436 | 11519.482 | -0.975% | [-2.313%, +0.292%] |
| 1 | 128 | 8 | rebuild | 394315.562 | 392738.875 | -0.246% | [-1.060%, +0.557%] |
| 1 | 128 | 8 | stream | 651653.875 | 533311.266 | -18.081% | [-18.571%, -17.684%] |
| 1 | 128 | 128 | one | 166.911 | 146.385 | -13.061% | [-14.689%, -11.785%] |
| 1 | 128 | 128 | page | 17162.975 | 17011.177 | -1.440% | [-3.160%, -0.225%] |
| 1 | 128 | 128 | rebuild | 591021.641 | 590211.828 | +0.403% | [-0.304%, +1.112%] |
| 1 | 128 | 128 | stream | 993801.719 | 821390.984 | -17.939% | [-19.283%, -17.172%] |
| 2 | 9 | 8 | one | 102.044 | 75.537 | -26.632% | [-31.721%, -22.833%] |
| 2 | 9 | 8 | page | 7297.213 | 7275.859 | -0.212% | [-1.156%, +0.658%] |
| 2 | 9 | 8 | rebuild | 245027.219 | 245002.891 | +0.057% | [-0.586%, +0.875%] |
| 2 | 9 | 8 | stream | 496699.062 | 373588.609 | -24.993% | [-26.167%, -24.047%] |
| 2 | 9 | 128 | one | 138.305 | 90.043 | -34.522% | [-35.398%, -33.586%] |
| 2 | 9 | 128 | page | 12397.865 | 12318.874 | -2.229% | [-6.271%, +0.335%] |
| 2 | 9 | 128 | rebuild | 427910.625 | 427462.781 | +0.064% | [-0.368%, +0.509%] |
| 2 | 9 | 128 | stream | 809991.375 | 643569.141 | -20.335% | [-20.880%, -19.796%] |
| 2 | 128 | 8 | one | 130.842 | 105.911 | -22.087% | [-28.035%, -16.339%] |
| 2 | 128 | 8 | page | 11526.883 | 11632.731 | +0.479% | [-0.410%, +1.316%] |
| 2 | 128 | 8 | rebuild | 391099.984 | 394274.719 | +0.863% | [+0.268%, +1.432%] |
| 2 | 128 | 8 | stream | 650688.141 | 534797.203 | -17.599% | [-18.036%, -17.139%] |
| 2 | 128 | 128 | one | 166.565 | 146.313 | -12.174% | [-13.612%, -10.403%] |
| 2 | 128 | 128 | page | 17004.701 | 17130.131 | +3.747% | [+0.276%, +10.374%] |
| 2 | 128 | 128 | rebuild | 588372.234 | 593719.312 | -0.674% | [-2.331%, +1.039%] |
| 2 | 128 | 128 | stream | 995773.422 | 825608.969 | -16.679% | [-17.194%, -16.019%] |

## Correctness and qualification

Forty scope corruptions (five coordinates, four storage-boundary lengths, two failure positions) now run through both single and batch rejection: 80 checks preserve scope-error precedence over sequence errors and unchanged log state. A new mixed single/batch test fills the complete replay bound, checks duplicates from single appends in later batches and from batches in later single appends, rejects late batch duplicates atomically, reconstructs through serde, accepts an empty append at the bound and rejects any further outcome without mutation. The warmed single-append allocation regression test fails against the baseline with one allocation and passes against the candidate with zero allocations/reallocations. Maximum-width scope rejection still passes all four fixture profiles and allows repaired appends.

Final source passes stable workspace unit/integration tests with all features (excluding the fuzz package), Rust 1.85.0 ports and SQLite unit/integration tests with all features, strict workspace all-target/all-feature Clippy, formatting and layer checks. Specifically, `cargo test --workspace --tests --all-features --exclude penelope-fuzz` and `cargo +1.85.0 test -p penelope-ports -p penelope-sqlite --tests --all-features` qualify the retained runtime; the other candidates' all-target smoke runs are not substituted for this final-source qualification. Graphify was updated after the Rust changes. No API, dependency, durable schema or wire format changes are introduced.

## Reproduction and evidence

Run `python3 docs/verification-data/2026-10-31-outcome-log-single-append/verify.py` from the repository root. It checks current-version sources and archived hashes, reconstructs each candidate from its pinned baseline and patch in a temporary directory, checks qualification/negative controls, and recomputes all 96 latency and 48 allocation comparisons from 2,304 timing rows and 768 allocation samples. It does not mutate production source or rerun latency measurements. Source checks require this version of the checkout.

For new timings, run the retained archived `build.py target/outcome-log-single-repeat`, then `measure.py target/outcome-log-single-repeat`, then `analyze.py target/outcome-log-single-repeat`, from an idle repository root with a fresh output directory. The build script temporarily substitutes baseline source and restores the checkout's candidate in `finally`; no other build, test or source operations should overlap it. To reproduce rejected implementations, use their recorded patches at the pinned baseline with the same harness. Timing executables are rebuilt rather than archived; measured binary hashes are retained. Archive assertions about 116 B of removed allocation traffic describe this measured toolchain/platform, not a cross-platform allocator guarantee.

[Evidence and verifier](verification-data/2026-10-31-outcome-log-single-append/).
