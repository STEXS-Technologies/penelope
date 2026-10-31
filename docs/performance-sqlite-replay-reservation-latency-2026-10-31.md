# Replay reservation latency — 2026-10-31

The replay page reservation change reduces allocation traffic, but this comparison does **not establish a repeatable latency improvement for 128-outcome pages**. All four full-page intervals (two histories, two campaigns) include zero. Retain the allocation improvement without translating it into an overall Penelope slowdown percentage.

## Method

Compared baseline `794914678621a18f44f770d261b0bd052974f51f` with current runtime source at `f15709ec8239044f41514ba63f22fc2d1ef35653`. Only `crates/penelope-sqlite/src/process.rs` was replaced between builds; benchmark, fixture, tests and all other runtime sources remained identical. The difference in that source is the exact replay result-vector reservation already documented in [the allocation report](performance-sqlite-replay-reservation-2026-10-31.md). Both versions passed the same complete 61-test process suite before measurement; the current source was restored after building.

Used the ordinary release `process_replay` benchmark without an allocation-counting allocator. Each fresh process creates histories of 1, 128 and 4,096 outcomes through public typed commits, then measures first, last, end, past-end and 128-limit page requests. Each profile warms 100 reads and times 1,000 public calls. Every result is checked against an independent history model outside its timed interval; retained database outcome counts are checked afterward.

Two campaigns each contain twelve balanced four-process blocks, alternating baseline/candidate/candidate/baseline and candidate/baseline/baseline/candidate. Total: 96 processes, 1,440 profile rows, 1,440,000 timed calls. The caller and inherited SQLite worker affinity were pinned to CPU 0 on an AMD Ryzen 9 7950X, Linux, Rust 1.99.0. Database directories were under the project qualification-temp directory on btrfs, not `/tmp` tmpfs. No local builds, tests or analysis ran during timing; background load and CPU frequency were not controlled.

Results use geometric candidate/baseline ratios within blocks and 20,000 deterministic block-bootstrap resamples. Negative means lower measured latency. Intervals are per-profile 95% intervals, without a multiple-comparison adjustment; these fifteen profiles are not fifteen independent replications of one effect. All profiles and both campaigns are retained below.

## Results

| Campaign | History | Profile | Baseline ns/call | Candidate ns/call | Change | 95% interval |
| --- | --- | --- | ---: | ---: | ---: | --- |
| 0 | 1 | first | 14557.8 | 14581.6 | +0.1634% | [-1.5345%, +2.7016%] |
| 0 | 1 | last | 14653.6 | 14439.0 | -1.4642% | [-3.7425%, +0.4599%] |
| 0 | 1 | end | 12581.0 | 12525.8 | -0.4388% | [-1.6546%, +0.5930%] |
| 0 | 1 | past | 12739.2 | 12597.3 | -1.1138% | [-5.0018%, +1.4691%] |
| 0 | 1 | page | 14972.0 | 14372.0 | -4.0074% | [-8.1266%, -1.3622%] |
| 0 | 128 | first | 15282.6 | 14723.3 | -3.6601% | [-8.1194%, -0.5752%] |
| 0 | 128 | last | 15228.0 | 14957.7 | -1.7748% | [-7.2782%, +3.5940%] |
| 0 | 128 | end | 12896.6 | 12944.7 | +0.3735% | [-3.3857%, +4.2808%] |
| 0 | 128 | past | 12705.1 | 12916.2 | +1.6613% | [+0.1034%, +3.5689%] |
| 0 | 128 | page | 179079.5 | 179739.8 | +0.3687% | [-0.7329%, +1.4008%] |
| 0 | 4096 | first | 15981.7 | 15580.3 | -2.5116% | [-8.9996%, +4.7610%] |
| 0 | 4096 | last | 15849.6 | 16179.9 | +2.0835% | [-2.7969%, +8.6975%] |
| 0 | 4096 | end | 13556.2 | 13788.6 | +1.7147% | [-3.3893%, +7.1712%] |
| 0 | 4096 | past | 13605.0 | 13759.2 | +1.1330% | [-4.4385%, +7.0432%] |
| 0 | 4096 | page | 180038.8 | 185237.5 | +2.8875% | [-0.6891%, +8.6470%] |
| 1 | 1 | first | 14480.8 | 14235.7 | -1.6931% | [-2.7337%, -0.6660%] |
| 1 | 1 | last | 14423.1 | 14187.4 | -1.6346% | [-2.4966%, -0.8631%] |
| 1 | 1 | end | 12414.4 | 12389.2 | -0.2034% | [-0.8769%, +0.4114%] |
| 1 | 1 | past | 12397.4 | 12389.1 | -0.0670% | [-0.7953%, +0.5824%] |
| 1 | 1 | page | 14451.4 | 14198.8 | -1.7481% | [-2.6994%, -0.8673%] |
| 1 | 128 | first | 14738.6 | 14446.7 | -1.9804% | [-2.8438%, -1.1464%] |
| 1 | 128 | last | 14990.8 | 14463.5 | -3.5176% | [-6.1858%, -1.7403%] |
| 1 | 128 | end | 12896.4 | 12617.1 | -2.1657% | [-6.7515%, +0.6644%] |
| 1 | 128 | past | 12847.7 | 12698.7 | -1.1595% | [-5.4430%, +2.0021%] |
| 1 | 128 | page | 177495.0 | 176964.5 | -0.2989% | [-0.9806%, +0.2871%] |
| 1 | 4096 | first | 15790.9 | 14644.4 | -7.2606% | [-13.1056%, -2.5439%] |
| 1 | 4096 | last | 16075.2 | 15249.2 | -5.1385% | [-9.5551%, -1.5982%] |
| 1 | 4096 | end | 13352.7 | 13184.5 | -1.2600% | [-3.1913%, +0.2840%] |
| 1 | 4096 | past | 13159.5 | 13156.9 | -0.0198% | [-0.8719%, +0.7659%] |
| 1 | 4096 | page | 177494.8 | 177937.6 | +0.2495% | [-0.5683%, +0.9802%] |

The one-outcome history with a page limit of 128 improved in both campaigns, as did the first-outcome profile at history 128. Several other single-row improvements appeared only in the second campaign. The past-end profile at history 128 regressed in the first campaign and did not repeat in the second. Empty-page allocation traffic is unchanged by the runtime change, so these timing variations also illustrate the limits of a small latency comparison. No full-page speedup or universal overhead improvement is claimed.

The allocation report remains the direct evidence: five fewer reallocations and 32,736 fewer requested Rust bytes per 128-row page; single-row pages request 792 fewer bytes. Requested allocation traffic is not peak memory and is separate from latency.

## Reproduction and qualification

[Raw evidence and scripts](verification-data/2026-10-31-replay-reservation-latency/) include all timing rows, exact source snapshots and hashes, binary hashes, test/build logs, metadata and deterministic analysis. Run `python3 docs/verification-data/2026-10-31-replay-reservation-latency/verify.py` to recompute and compare all thirty summaries and check both process-suite results. The build script temporarily replaces production source and restores it in `finally`; run it only from a clean, idle checkout. Timing and analysis scripts operate in their own directory.

The archived CI result for `8b6ff6d` is successful and covers the reservation runtime change. It does not attest to the later test-only boundary commit or to this measurement report. No production code changed for this report.
