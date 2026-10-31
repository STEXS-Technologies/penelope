# Cached SQLite process payload reads, 2026-10-31

Reusing the payload-read SELECT statement reduced warmed public read latency in both measured campaigns. Healthy empty and 32-byte reads improved 13.87–15.83%, 4 KiB reads 11.99–13.01%, and 1 MiB reads 0.32–0.54%. Each read adds one Rust allocation requesting 80 bytes. This tradeoff is retained for the repeatable latency gain; it is not a claim about total application overhead or total heap use.

`read_payload` now uses rusqlite's existing bounded statement cache. Every call still binds its scope/digest, reads current stored bytes, verifies BLOB storage class and configured size, hashes those bytes against the requested digest, and returns an owned copy only after verification. Missing or invalid stored content remains Invariant. The statement is cached; payload bytes and validation answers are not. Writes, transactions, worker dispatch and cache capacity are unchanged.

## Ordinary latency

Baseline is `ac4dcde5484174faaa039937b6421e2bde1944fa`. The unchanged `process_payloads` release benchmark exercises five public operations for 0, 32, 4096 and 1048576 bytes. Each profile warms up 100 calls then measures 1,000 calls, except 1 MiB profiles warm up 32 and measure 128. Each result is checked outside timing. Fixture checks verify stored bytes and usage, reject corruption, then repair and recheck read and duplicate put. Corrupt empty content is represented by a one-byte damaged row. Missing reads use a distinct absent digest, so the fixture size does not represent a missing payload actually being read.

Two campaigns each contain eight balanced ABBA/BAAB blocks of four processes: 64 processes, 1,280 timing rows and 1,000,960 measured operations (600,576 read/missing/corrupt-read operations). Timing includes the public asynchronous worker roundtrip. Caller and worker were pinned to CPU 0 on a Ryzen 7950X, Linux 7.2.9, with databases on project NVMe btrfs storage. No project builds, tests or analysis ran during timing. System background load and CPU frequency were uncontrolled.

Changes use geometric candidate/baseline ratios within blocks, with 20,000 deterministic block-bootstrap samples for each interval. Negative means faster. Every changed read profile has an interval below zero in both campaigns. Unchanged duplicate and corrupt-put paths are included as controls; their occasional one-campaign differences are not claimed as improvements or regressions. Intervals are per-profile and are not adjusted for multiple comparisons.

| Campaign | Fixture bytes | Operation | Latency change | Bootstrap 95% interval |
| --- | ---: | --- | ---: | --- |
| 1 | 0 | duplicate | +0.2082% | [-0.1818%, +0.6289%] |
| 1 | 0 | read | -13.8653% | [-14.1523%, -13.5897%] |
| 1 | 0 | missing | -14.7815% | [-15.3671%, -14.2978%] |
| 1 | 0 | corrupt_read | -15.3980% | [-17.2429%, -14.3593%] |
| 1 | 0 | corrupt_put | -0.5726% | [-1.3159%, +0.0031%] |
| 1 | 32 | duplicate | -1.2242% | [-3.7673%, +0.7649%] |
| 1 | 32 | read | -15.8295% | [-18.3767%, -14.3979%] |
| 1 | 32 | missing | -16.1755% | [-18.3893%, -14.7987%] |
| 1 | 32 | corrupt_read | -15.0792% | [-16.4854%, -13.9566%] |
| 1 | 32 | corrupt_put | -0.7573% | [-2.1138%, +0.1916%] |
| 1 | 4096 | duplicate | -0.8305% | [-2.8855%, +0.4627%] |
| 1 | 4096 | read | -13.0105% | [-14.7190%, -11.9644%] |
| 1 | 4096 | missing | -15.7081% | [-17.7389%, -14.5973%] |
| 1 | 4096 | corrupt_read | -12.1779% | [-12.8479%, -11.6046%] |
| 1 | 4096 | corrupt_put | +0.0886% | [-0.4331%, +0.6123%] |
| 1 | 1048576 | duplicate | +0.4258% | [-0.1149%, +1.1541%] |
| 1 | 1048576 | read | -0.5378% | [-0.9624%, -0.1809%] |
| 1 | 1048576 | missing | -15.3689% | [-18.2493%, -13.4842%] |
| 1 | 1048576 | corrupt_read | -0.8910% | [-1.7225%, -0.2834%] |
| 1 | 1048576 | corrupt_put | -0.6679% | [-1.6274%, -0.0666%] |
| 2 | 0 | duplicate | +0.1499% | [-0.5193%, +0.9049%] |
| 2 | 0 | read | -13.9945% | [-14.4926%, -13.5671%] |
| 2 | 0 | missing | -14.4780% | [-15.0323%, -13.9073%] |
| 2 | 0 | corrupt_read | -14.5366% | [-14.8928%, -14.1488%] |
| 2 | 0 | corrupt_put | -0.3253% | [-0.8808%, +0.2533%] |
| 2 | 32 | duplicate | +0.7672% | [+0.2354%, +1.3089%] |
| 2 | 32 | read | -14.7047% | [-15.0578%, -14.3657%] |
| 2 | 32 | missing | -15.4521% | [-15.9410%, -15.0108%] |
| 2 | 32 | corrupt_read | -14.7349% | [-15.1783%, -14.2922%] |
| 2 | 32 | corrupt_put | -0.0970% | [-0.5344%, +0.3815%] |
| 2 | 4096 | duplicate | +0.0585% | [-0.6535%, +0.8128%] |
| 2 | 4096 | read | -11.9868% | [-12.2534%, -11.6670%] |
| 2 | 4096 | missing | -14.7423% | [-15.1735%, -14.3108%] |
| 2 | 4096 | corrupt_read | -12.1417% | [-12.5416%, -11.6938%] |
| 2 | 4096 | corrupt_put | +0.0358% | [-0.4811%, +0.6338%] |
| 2 | 1048576 | duplicate | +0.0137% | [-0.2867%, +0.3388%] |
| 2 | 1048576 | read | -0.3228% | [-0.4471%, -0.2052%] |
| 2 | 1048576 | missing | -15.0476% | [-15.3861%, -14.6936%] |
| 2 | 1048576 | corrupt_read | -0.5071% | [-0.6909%, -0.3168%] |
| 2 | 1048576 | corrupt_put | -0.1140% | [-0.2777%, +0.0594%] |

## Rust allocation traffic

The unchanged separate allocation benchmark ran four processes per revision, 16 measured samples per profile after 16 warmups: 1,280 samples per revision, 2,560 total. In all three read cases at every size, candidate counts rise by exactly one allocation and 80 requested bytes, with reallocations unchanged. Healthy nonempty reads additionally allocate their returned bytes in both revisions. For example, healthy 32-byte reads change from 7 allocations / 4 reallocations / 624 requested bytes to 8 / 4 / 704; missing reads change from 6 / 4 / 592 to 7 / 4 / 672. Unchanged put profiles retain identical measured tuples. These counters include Rust allocator traffic during the public call, including worker work; they exclude SQLite C allocations. Requested traffic includes reallocations and is neither live nor peak memory.

## Validation

Both revisions passed all 61 process integration tests before measurement. Existing payload tests warm reads and then change content through a second connection, covering shorter, empty, substituted, oversized, TEXT, INTEGER and REAL values; they require current-row rejection, preserved owned buffers, unchanged usage and rows, repair, and reopened reads. Boundary tests cover empty and 1 MiB payloads. No test change was necessary for this statement-only reuse.

The retained candidate passed full workspace all-target/all-feature tests excluding the fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting and layer checks. SQLite suites comprise 35 unit, 61 process and 45 task tests. Debug benchmark output from qualification is not used as latency evidence.

## Evidence and reproduction

[Evidence](verification-data/2026-10-31-payload-read-cache/) includes raw timing/allocation rows, all summaries, qualification logs, baseline/candidate source snapshots, source/binary hashes and scripts. Run `python3 verify.py` within it to validate counts, allocation changes and recomputed timing summaries.

To reproduce, use a disposable checkout of the baseline, copy this directory into it, and run `python3 <directory>/build.py` from the repository root. Then run `timing.py` and `summarize.py` there. The builder temporarily changes production source and restores the candidate afterward; use an isolated checkout. Binaries are not committed. Matching binary hashes requires the same build toolchain and environment. Release ordinary benchmarks provide latency; instrumented allocation probes provide traffic only.
