# Cached task blob SELECT — 2026-10-31

`with_verified_blob`, used by public `SqliteTaskJournal::read_blob`, now prepares its existing scoped blob SELECT with `prepare_cached`. Reusing the compiled SQL reduces warmed read latency for small blobs. Scope/digest bindings are supplied anew on every call. Stored type, length, digest and caller-buffer checks remain, and verification still precedes any caller-buffer write. No blob bytes or verification results are retained across operations.

This has a measured allocation tradeoff: one additional Rust allocation requesting 80 bytes per read. The previous direct read path made no temporary Rust allocation in this fixture; the new path trades a cache-key allocation for lower SQL preparation cost. The port uses caller-owned output buffers and does not promise that the SQLite implementation as a whole is heap-free. No whole-application speedup or overall Penelope overhead percentage follows from these measurements.

## Method

Baseline is `dfbdc294929019939939689209149905b833910c`. Only the `prepare` to `prepare_cached` change in `src/tasks.rs` differs between builds; benchmarks, fixtures, tests and other runtime source are identical. Both versions passed the same complete 42-test task integration suite before measurement, and candidate source was restored afterward.

Used unchanged ordinary release `task_blobs` and separate `task_blob_allocations` benchmarks. Each profile opens a fresh journal and publicly commits its typed intent and blob. Healthy and externally corrupted stored blobs are tested at declared sizes 0, 32, 4,096 and 1,048,576 bytes. Corrupt nonempty blobs have equal-length wrong contents; the corrupt zero-byte reference has one stored byte and fails length validation. Sixteen warmups precede each measured profile. The timing loop checks the expected typed result on every read; caller-buffer contents and retained stored bytes are checked outside timing. Corrupt caller buffers must retain their sentinel bytes.

Two campaigns each contain twelve balanced four-process ABBA/BAAB blocks: 96 ordinary release processes, 768 profile rows and 600,576 measured reads. Each small profile performs 1,000 reads; each 1 MiB profile performs 128. The caller runs on CPU 0 of an AMD Ryzen 9 7950X, Linux, Rust 1.99.0, with btrfs project qualification-temp databases. No local builds, tests or analysis ran during measurement. Background load and CPU frequency were not controlled. This measures warmed reads of retained fixture blobs, not initial preparation, arbitrary histories or full application workloads.

Per-block geometric candidate/baseline ratios use deterministic 20,000-resample block-bootstrap 95% intervals. Negative means lower latency. These are per-profile intervals without multiplicity correction; all profiles and campaigns are retained.

## Latency

| Campaign | Declared bytes | Corrupt | Baseline ns/read | Candidate ns/read | Change | 95% interval |
| --- | ---: | --- | ---: | ---: | ---: | --- |
| 0 | 0 | false | 6206.9 | 4939.3 | -20.4218% | [-20.7592%, -20.0397%] |
| 0 | 0 | true | 6180.4 | 4921.3 | -20.3724% | [-20.8453%, -19.9689%] |
| 0 | 32 | false | 6198.8 | 4973.5 | -19.7670% | [-20.0496%, -19.4568%] |
| 0 | 32 | true | 6184.5 | 4966.8 | -19.6898% | [-19.9424%, -19.4509%] |
| 0 | 4096 | false | 8044.4 | 6844.7 | -14.9139% | [-16.5730%, -12.7055%] |
| 0 | 4096 | true | 7952.0 | 6708.5 | -15.6368% | [-15.9229%, -15.3775%] |
| 0 | 1048576 | false | 438260.3 | 436623.1 | -0.3736% | [-0.5412%, -0.2038%] |
| 0 | 1048576 | true | 423450.7 | 422306.0 | -0.2703% | [-0.5680%, +0.0267%] |
| 1 | 0 | false | 6173.8 | 4947.2 | -19.8687% | [-20.3121%, -19.4563%] |
| 1 | 0 | true | 6166.1 | 4925.0 | -20.1273% | [-20.6752%, -19.6018%] |
| 1 | 32 | false | 6202.2 | 4969.6 | -19.8739% | [-20.3646%, -19.4986%] |
| 1 | 32 | true | 6167.7 | 4954.7 | -19.6662% | [-19.9633%, -19.3214%] |
| 1 | 4096 | false | 8011.2 | 6763.9 | -15.5698% | [-15.8123%, -15.3600%] |
| 1 | 4096 | true | 8032.9 | 6713.8 | -16.4208% | [-18.1669%, -15.3767%] |
| 1 | 1048576 | false | 437951.8 | 436402.4 | -0.3538% | [-0.4442%, -0.2635%] |
| 1 | 1048576 | true | 422993.5 | 421514.9 | -0.3495% | [-0.4124%, -0.2847%] |

Healthy small reads improve in both campaigns: about 20% for 0/32 bytes, 14.91–15.57% at 4 KiB, and 0.35–0.37% at 1 MiB. Corrupt 1 MiB reads have a first-campaign interval including zero, so no repeatable corrupt-large-blob speedup is claimed.

## Allocations and correctness

All 1,024 allocation samples agree by revision: baseline `(0 allocations, 0 reallocations, 0 requested bytes)`, candidate `(1, 0, 80)`, in every profile. Counters cover Rust `System` requests, exclude SQLite C allocations, and do not measure peak/retained memory or the compiled statement's C allocation footprint.

Existing blob-column tests cover ten stored-row representations/faults, unchanged caller buffers and durable tables on failure, repair, duplicate commit verification and reconstruction. Existing statement reuse tests alternate scopes with the same digest and repeatedly delete/repair blobs while creating/dropping a schema index. They now exercise reuse of the public-read SELECT as well as reconstruction. The benchmark independently checks healthy and corrupt buffers and stored bytes. All existing tests are unchanged.

Stable workspace tests with all features (excluding fuzz), including unit/integration/doc tests, and SQLite unit/integration tests on Rust 1.85.0 passed. The SQLite suites contain 34 unit, 61 process and 42 task tests. Strict workspace Clippy covers all targets/features; formatting, layer boundaries and the local graph update passed. The two release benchmark targets were built for both variants. Other benchmark binaries were not rerun as qualification for this change.

[Evidence and reproduction scripts](verification-data/2026-10-31-task-blob-cache/) retain all allocation/timing rows, exact source snapshots/hashes, benchmark/binary hashes, build/test logs and analysis. Run `python3 docs/verification-data/2026-10-31-task-blob-cache/verify.py` to check all samples and independently recompute sixteen timing summaries. The build script temporarily replaces source and restores it in `finally`; use an idle checkout.
