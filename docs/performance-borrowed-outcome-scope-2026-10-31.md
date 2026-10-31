# Borrowed outcome scope validation — 2026-10-31

Replay now uses the existing checked `OutcomeReplayPage` constructor to validate
outcome schema and all five scope fields, rather than constructing an owned
scope for an additional check on each decoded row. Bounded column access,
checksum verification, sequence/frontier validation, the read transaction,
continuation checks and owned return values remain. A malformed scope can be
rejected later within the bounded page; no partial page is returned.

The domain `ProcessOutcome::validate_for_scope` method also compares its five
fields by reference. It preserves its validation and typed mismatch error while
avoiding four string clones for heap-backed identities. Public signatures,
serialized data, SQL, schema and durability settings are unchanged.

## What improved

At maximum identifier length (128 bytes), a 128-outcome public replay call
removed **512 Rust allocations and 65,536 requested bytes**: 1,037 → 525
allocations and 166,680 → 101,144 requested bytes. A one-outcome call removed
four allocations and 512 requested bytes. Empty calls and the measured short
identities retained their allocation counts. Reallocation counts stayed zero.
These are allocation-traffic measurements across Rust threads, excluding
SQLite's C heap; they are not peak memory or RSS reductions.

Short-identifier 128-outcome reads improved **1.13–2.03%** across both history
sizes and both campaigns; all four corresponding intervals excluded zero.
Maximum-width pages did not show a repeated latency improvement or regression.
Other conditions varied. Three empty-read conditions had positive intervals in
one campaign without repeating for the same condition in the other campaign.
This is a replay-path result, not a new overall application-overhead percentage.

## Measurement

Baseline: `cbfe5c5f0d840cbd1ad2a4ba119a5883a9ab1c13`. Both variants use the same
updated benchmark harness. Its default scope remains unchanged; setting
`PENELOPE_REPLAY_LONG_SCOPE=1` selects four 128-byte scope identifiers. Invalid
option values fail rather than silently switching fixtures.

The ordinary `process_replay` release benchmark populates fresh WAL/FULL
databases through public commits. It tests histories of 1, 128 and 4,096 outcomes,
first/last/end/past-end cursors with limit 1, and a page with limit 128. At
history 1, the page returns one outcome; first/last are equivalent requests.
Each profile warms 100 reads and times 1,000 public calls. Exact returned rows,
scope, cursors and continuation are verified outside timing. Retained database
outcome counts are checked afterward. Setup, opening and verification are not
timed. All measured pages contain the same business facts in both variants.

Two campaigns each use six balanced four-process blocks (ABBA/BAAB), testing
short and long scopes at every slot: 96 processes, 1,440 profile rows,
1,440,000 timed calls and 144,000 warmup calls for the final candidate. A separate
counting-allocator executable supplies 320 allocation samples, not latency.
Caller and worker inherit CPU 0 affinity on an AMD Ryzen 9 7950X, Linux 7.2.9,
Rust 1.99.0. Databases use the project's btrfs qualification directory. No project
builds, tests, graph jobs or analysis run during ordinary timing. Background load
and CPU frequency remain uncontrolled.

Changes below are geometric candidate/baseline ratios of within-block geometric
means. Each interval uses 20,000 deterministic six-block bootstrap samples.
Baseline/candidate columns are process medians in nanoseconds per public call;
their ratio need not equal the paired estimate. These are exploratory,
per-profile 95% intervals without multiple-comparison adjustment, not a
production latency guarantee, concurrency result or crash/power-loss test.

## All final latency profiles

| Campaign | Scope | History | Profile | Baseline ns | Candidate ns | Change | 95% interval |
| --- | --- | ---: | --- | ---: | ---: | ---: | --- |
| 0 | long | 1 | end | 12284.1 | 12295.0 | -0.0591% | [-0.6534%, +0.3479%] |
| 0 | long | 1 | first | 14817.3 | 14925.3 | +0.6714% | [-0.1763%, +1.7407%] |
| 0 | long | 1 | last | 14807.5 | 14865.4 | +0.3739% | [-0.3251%, +1.2825%] |
| 0 | long | 1 | page | 14801.6 | 14898.5 | +0.4330% | [-0.5701%, +1.4034%] |
| 0 | long | 1 | past | 12291.3 | 12370.1 | +0.2696% | [-1.3630%, +1.4549%] |
| 0 | long | 128 | end | 12672.7 | 12651.1 | +0.2057% | [-0.3664%, +0.8970%] |
| 0 | long | 128 | first | 15213.8 | 15142.5 | -0.5467% | [-1.5838%, +0.3581%] |
| 0 | long | 128 | last | 15270.4 | 15122.5 | -1.0110% | [-1.3127%, -0.6176%] |
| 0 | long | 128 | page | 266354.1 | 266671.9 | +0.1325% | [-0.0770%, +0.2748%] |
| 0 | long | 128 | past | 12656.3 | 12712.9 | +0.1724% | [-0.2809%, +0.6135%] |
| 0 | long | 4096 | end | 13223.4 | 13317.4 | +0.3437% | [-0.4035%, +1.0660%] |
| 0 | long | 4096 | first | 15632.1 | 15543.6 | -0.8001% | [-3.8271%, +2.2461%] |
| 0 | long | 4096 | last | 16099.5 | 16200.2 | +0.5233% | [-0.4432%, +1.2553%] |
| 0 | long | 4096 | page | 267521.4 | 267699.2 | -0.1840% | [-0.6791%, +0.2036%] |
| 0 | long | 4096 | past | 13228.5 | 13274.4 | +0.0739% | [-0.7678%, +1.0024%] |
| 0 | short | 1 | end | 12335.5 | 12292.8 | -0.8411% | [-2.4247%, +0.4254%] |
| 0 | short | 1 | first | 14236.9 | 14022.8 | -1.0590% | [-1.6359%, -0.4617%] |
| 0 | short | 1 | last | 14295.1 | 13978.7 | -4.9162% | [-11.0248%, -1.2127%] |
| 0 | short | 1 | page | 14178.9 | 14051.4 | -1.1980% | [-1.6898%, -0.6761%] |
| 0 | short | 1 | past | 12377.6 | 12280.3 | -0.3850% | [-0.8693%, +0.1918%] |
| 0 | short | 128 | end | 12626.3 | 12535.5 | -0.2460% | [-0.8291%, +0.3367%] |
| 0 | short | 128 | first | 14453.3 | 14270.7 | -4.3042% | [-9.5741%, -1.1572%] |
| 0 | short | 128 | last | 14452.3 | 14321.9 | -3.2980% | [-8.5150%, +0.1086%] |
| 0 | short | 128 | page | 173793.7 | 170475.4 | -1.9137% | [-2.0836%, -1.7081%] |
| 0 | short | 128 | past | 12599.7 | 12516.4 | +0.2415% | [-0.5981%, +1.2556%] |
| 0 | short | 4096 | end | 13045.4 | 13135.4 | +0.0089% | [-0.5682%, +0.4697%] |
| 0 | short | 4096 | first | 14540.1 | 14446.5 | -1.9217% | [-3.8277%, -0.6216%] |
| 0 | short | 4096 | last | 15110.1 | 15086.5 | -0.1505% | [-0.8246%, +0.4643%] |
| 0 | short | 4096 | page | 174192.9 | 170933.5 | -2.0343% | [-2.4649%, -1.7373%] |
| 0 | short | 4096 | past | 13058.0 | 13111.6 | +0.4860% | [+0.2587%, +0.7138%] |
| 1 | long | 1 | end | 12343.7 | 12271.9 | +0.3606% | [-0.3777%, +1.1500%] |
| 1 | long | 1 | first | 14893.1 | 14786.9 | -1.0572% | [-2.8769%, +0.3528%] |
| 1 | long | 1 | last | 14881.1 | 14751.0 | -0.7106% | [-1.7322%, +0.3751%] |
| 1 | long | 1 | page | 14867.4 | 14669.0 | -1.3983% | [-2.9521%, +0.0419%] |
| 1 | long | 1 | past | 12345.3 | 12184.4 | -0.8878% | [-2.0867%, +0.4945%] |
| 1 | long | 128 | end | 12592.9 | 12634.1 | +0.3790% | [-0.7508%, +1.7460%] |
| 1 | long | 128 | first | 15275.5 | 15037.9 | -1.1895% | [-2.2070%, -0.0273%] |
| 1 | long | 128 | last | 15318.8 | 15096.0 | -1.9014% | [-2.8227%, -1.0485%] |
| 1 | long | 128 | page | 266896.7 | 267088.4 | -2.2221% | [-6.2347%, +0.1086%] |
| 1 | long | 128 | past | 12587.9 | 12640.1 | +0.6230% | [-1.2009%, +2.5580%] |
| 1 | long | 4096 | end | 13342.9 | 13239.5 | -1.5381% | [-2.9069%, -0.0711%] |
| 1 | long | 4096 | first | 15609.3 | 15397.1 | -1.5231% | [-2.1754%, -0.9069%] |
| 1 | long | 4096 | last | 16321.2 | 16040.2 | -1.6659% | [-2.1357%, -1.2010%] |
| 1 | long | 4096 | page | 268643.1 | 265993.6 | -0.7660% | [-1.3277%, -0.1636%] |
| 1 | long | 4096 | past | 13396.6 | 13327.5 | -1.0065% | [-2.8655%, +1.0189%] |
| 1 | short | 1 | end | 12205.3 | 12336.3 | +0.8987% | [+0.2504%, +1.4778%] |
| 1 | short | 1 | first | 14086.4 | 14095.9 | +0.2967% | [-0.1739%, +0.7303%] |
| 1 | short | 1 | last | 14009.7 | 14015.3 | +0.1820% | [-0.2563%, +0.6110%] |
| 1 | short | 1 | page | 14064.9 | 14118.5 | -0.1329% | [-0.8553%, +0.5279%] |
| 1 | short | 1 | past | 12209.4 | 12367.4 | +0.8343% | [+0.4633%, +1.2454%] |
| 1 | short | 128 | end | 12489.9 | 12534.8 | +0.1241% | [-0.5629%, +0.7388%] |
| 1 | short | 128 | first | 14364.7 | 14313.6 | +0.4712% | [-1.6629%, +2.3623%] |
| 1 | short | 128 | last | 14405.2 | 14336.5 | +1.6724% | [-1.3168%, +6.4367%] |
| 1 | short | 128 | page | 173004.9 | 170493.0 | -1.5986% | [-1.9537%, -1.2188%] |
| 1 | short | 128 | past | 12523.2 | 12521.4 | +0.6552% | [-0.6004%, +1.9876%] |
| 1 | short | 4096 | end | 13019.2 | 13009.3 | -0.0438% | [-0.9954%, +0.8229%] |
| 1 | short | 4096 | first | 14593.3 | 14540.8 | -0.4606% | [-1.0788%, +0.1937%] |
| 1 | short | 4096 | last | 15148.5 | 15063.3 | -0.4576% | [-1.8554%, +0.9642%] |
| 1 | short | 4096 | page | 173473.9 | 170692.5 | -1.1287% | [-1.7349%, -0.2915%] |
| 1 | short | 4096 | past | 13043.0 | 12965.2 | -0.5345% | [-1.3544%, +0.2674%] |

## Earlier candidates retained separately

The first candidate used the allocation-free domain method for the extra
per-row scope check. It removed the same allocations, but maximum-width
128-outcome pages regressed approximately 0.54–0.90% across both campaigns.
The next candidate added an inline hint: maximum-width pages became broadly
neutral, while several small-read conditions regressed. Neither implementation
was retained. Their raw rows, summaries, source patches and binary/source hashes
are under `excluded/` in the evidence directory. These exploratory trials helped
select the final implementation and do not count as independent confirmation of
its latency estimate. Across all three variants, 288 ordinary timing processes
and all 4,320 timing rows are retained.

## Integrity and qualification

A domain test compares borrowed validation with the legacy owned-scope equality
for all 32 combinations of scope substitutions at identifier lengths 5, 22, 23,
24, 25 and 128 (192 cases). The isolated allocation test requires zero allocations
for a successful maximum-width validation; baseline code fails with four.

A SQLite test changes each of the five scope fields in the first and last rows
of a two-row page using maximum-width identifiers. It recomputes valid checksums
so the scope validator itself must reject all ten substitutions. Each rejected
read preserves durable rows and an earlier owned page; repaired records become
readable through the warmed store. Temporarily disabling the page constructor's
scope check makes the test fail. The original check was restored before final
qualification.

The full workspace all-target/all-feature suite (excluding the fuzz package)
passed. Domain and SQLite all-target/all-feature tests passed on Rust 1.85.0.
After test-fixture cleanup, final domain all-target tests and all 64 process
integration tests passed again on both toolchains. Strict workspace Clippy,
formatting and layer checks passed. Graphify received an AST-only update.
Production and benchmark source hashes match the final measured candidate.

## Evidence and reproduction

[Evidence](verification-data/2026-10-31-borrowed-outcome-scope/) includes compressed
raw outputs, all summaries, patches, source/binary hashes, scripts and qualification
logs. From the repository root, run:

```sh
python3 docs/verification-data/2026-10-31-borrowed-outcome-scope/verify.py
```

For new measurements, use the checkout containing this change and an idle
worktree. Run the archived `build.py`, `measure.py` and `analyze.py` in that order,
passing the same fresh output directory under `target/` to each. The build script
temporarily substitutes baseline production files and restores them in `finally`;
do not run other project jobs concurrently. The measure script refuses existing
timing output. Compare the new metadata with the archived hashes and environment
before interpreting differences.
