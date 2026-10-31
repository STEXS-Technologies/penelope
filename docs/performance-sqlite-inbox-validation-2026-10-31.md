# Verified byte-match admission for duplicate inputs — 2026-10-31

Baseline: `feb2234ef4c27f502c4790f767530be2773cf665`. Candidate: `3f8e1abf41491f06f8966eead1eb913b39bbef5e`.

Input admission previously copied the retained input/checksum and decoded the stored JSON on every duplicate request. It now uses a cached SELECT, borrows bounded BLOBs from its current row, and compares the retained bytes to the bytes just serialized from the validated request. A byte match is accepted only after its durable checksum is verified. Different bytes still go through the existing checksum/decode/equality path, preserving valid alternative JSON encodings and the distinction between conflicts and damaged evidence.

Serialization and its incoming-record bound still happen before the lookup. Stored record/checksum bounds and SQLite storage-class checks happen before either duplicate path. Every operation rereads its current row within the existing immediate transaction; no decoded input, durable bytes or validation result is retained across calls. New-input count/usage charging and insertion remain after the lookup and are unchanged. Schema and public APIs are unchanged.

## Ordinary release latency

The ordinary timing executable shares the typed fixture with the allocation probe but does not install the counting allocator. Each profile uses a fresh temporary database, a publicly admitted input or a publicly committed consumed input, 64 warmup duplicate admissions and 5,000 measured admissions. Every returned receipt must be marked duplicate; final durable input/consumption/commit/outcome counts are checked.

Two campaigns each run 12 four-process blocks, alternating ABBA and BAAB revision order, for 96 isolated processes and 960,000 measured calls altogether. Both pending and consumed profiles run in every process, in that order. All samples are retained. Caller and database worker inherit CPU 0 affinity. Storage is the project qualification directory on NVMe btrfs, not /tmp tmpfs. The archived platform/toolchain describe this Linux/Rust release run; background system load and CPU frequency are not controlled. No own builds, tests, graph updates or allocation probes ran during timing.

For each block/profile, the analysis compares the geometric mean of its two candidate times per call with its two baseline times. Reported changes are geometric means across all 12 block ratios. Confidence intervals use 20,000 deterministic block-bootstrap resamples, taking the 2.5th and 97.5th percentiles; negative changes mean lower latency.

| Campaign | Input state | Baseline ns/call | Candidate ns/call | Change | 95% interval |
|---|---|---:|---:|---:|---:|
| 1 | pending | 12064.8 | 9875.8 | -18.14381% | [-21.48051%, -14.57106%] |
| 1 | consumed | 12119.9 | 9977.0 | -17.68080% | [-21.53272%, -13.92279%] |
| 2 | pending | 11918.6 | 9650.3 | -19.03209% | [-20.09265%, -18.21764%] |
| 2 | consumed | 11964.7 | 9806.0 | -18.04256% | [-21.02590%, -14.58863%] |

All four intervals are below zero. The result is faster warm canonical duplicate admission in these fixtures, not an exact whole-application slowdown percentage or a claim about first admission, noncanonical fallback, arbitrary storage, different core placements or all workloads. The measured improvement combines byte matching, borrowed BLOBs and SELECT statement reuse; the experiment does not separately attribute their timing contributions.

## Allocation evidence

Four release allocation-probe processes per revision, two profiles and 32 samples per profile after 64 warmups produce 512 samples. Every sample has the same tuple for its revision/profile; none are excluded. Both pending and consumed canonical duplicates change from 9 to 8 Rust allocations and 1,084 to 968 requested bytes per call, with one reallocation unchanged. This is one fewer allocation and 116 fewer requested bytes per call.

The counter observes Rust System allocation/reallocation requests across caller and worker threads. SQLite's C allocator and peak/live memory are not measured. These instrumented runs are separate from ordinary timing. Their requested-byte totals are allocation traffic, not retained memory.

## Compatibility and failure qualification

Two integration tests cover pending and consumed inputs with canonical, pretty-printed and escaped JSON; valid but conflicting payload identity; damaged, short, oversized and wrong-storage-class checksums; oversized and wrong-storage-class payloads; and malformed JSON with a valid checksum. Alternate encodings independently deserialize to the original input and must remain accepted. Both a warmed worker and a reopened worker must produce the expected duplicate receipt, Conflict or Invariant result, with exact snapshots of all ten process/lease-counter tables unchanged.

A second test pads otherwise identical JSON with whitespace to exactly a configured 4,096-byte stored-record limit and one byte beyond it. The exact-limit record remains a duplicate; the oversized record rejects with Invariant without changing rows. Twenty isolated regression processes, ten stable and ten Rust 1.85.0, pass both tests: 40 test executions and 480 fixtures. The complete 57-test process suite also passes against baseline and candidate with these same additions.

A mutation control bypasses only the byte-match checksum comparison. A canonical retained input with a damaged checksum then incorrectly returns a duplicate receipt and the test fails. The control stops at that first failure; it does not execute the entire faulty matrix. Source restoration is guaranteed by `finally`, and both restored tests pass. The unchanged baseline passes both tests; the optimization preserves its compatibility and rejection behavior.

Stable workspace all-target/all-feature tests excluding the fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting, layer boundaries and graphify update pass. Both compilers pass 33 SQLite unit, 57 process and 42 task tests. No fresh local fuzz or optional StateChronicle conformance run is claimed. Debug benches executed during all-target qualification are functional checks and are excluded from the controlled timing results.

[Evidence and reproduction](verification-data/2026-10-31-inbox-validation/README.md) includes sources, hashes, raw timing/allocation samples, metadata and bootstrap analysis, intentional-control logs, repeated regressions, qualification logs and a standalone verifier. The fixtures do not exhaust all interleavings, corruption recovery or power-loss behavior. Release plans from Downloads are not included.
