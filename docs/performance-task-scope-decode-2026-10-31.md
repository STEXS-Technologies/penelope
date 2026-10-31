# SQLite task replay scope decoding — 2026-10-31

Code: `7433d8de3e2c0707ce9a6378da91b7e11a5ebd06`. Baseline: `bd187069a30b0c3113089c83260c4b961db45ebe`.

Replay now compares each canonical record's full scope encoding with the expected scope once prepared per replay, avoiding repeated deserialization of its 128 digest bytes. It still hashes every complete original record, strictly decodes the remaining fields, applies every event, verifies referenced blobs, and validates the derived head and raw-byte usage. External-commit detection and the transaction-local one-entry BlobRef memo are retained.

The decoder couples the expected scope with its encoding and uses fixed 1 KiB scope and 4 KiB record scratch buffers. It replaces only an exactly matching scope object with `null` in temporary JSON. Duplicate/unknown fields and malformed tails remain errors. Pretty or reordered fields, escaped keys, sequence-form JSON, and records exceeding scratch capacity use the original decoder. Stored bytes, quotas, wire format, schema, dependencies, and public API do not change. Current TaskScope encodings fit the scope buffer; all 256 repeated digest-byte patterns are tested. Scratch buffers add 5 KiB of replay-local stack storage.

## Method

AMD Ryzen 9 7950X, CPU 0 affinity, Linux 7.2.9, Rust 1.99 release builds. No concurrent project builds/tests or analysis ran during measured campaigns. Each primary campaign has 24 alternating AB/BA pairs, 480 isolated processes, 19 latency comparisons and 9,216 Rust allocation samples. Reconstruction tests histories of 2/16/128 records with repeated/alternating checkpoints and canonical/pretty JSON; each latency observation contains 250 already-reserved public authorization retries. Commit observations contain 128 appends. Fixture setup, formatting, checksumming, and head rebuilding are outside timing. The external-write profile commits through another connection before each append, forcing history replay; it is a stress profile, not typical single-writer behavior.

Changes below are mean paired candidate/baseline percentage changes, with deterministic 20,000-resample percentile bootstrap 95% intervals. Negative means faster. Every pair is retained. These are operation-specific measurements, not an overall Penelope slowdown estimate. Intervals describe these runs on this host, not all machines or workloads.

## Final primary results

| Operation | History/blob size | Change | 95% interval |
|---|---:|---:|---:|
| commit, ordinary | 0 | -0.20923% | [-3.10832, +1.96239]% |
| commit, ordinary | 32 | +0.64076% | [-0.11219, +1.40306]% |
| commit, ordinary | 4096 | +0.84492% | [-0.22771, +2.17705]% |
| commit, ordinary | 1048576 | +0.29369% | [-0.15635, +0.80431]% |
| commit, forced replay | 0 | -32.12950% | [-33.28667, -30.18693]% |
| commit, forced replay | 32 | -32.47795% | [-32.87338, -32.00938]% |
| commit, forced replay | 4096 | -32.25596% | [-32.51596, -32.00129]% |
| retry, canonical, repeated | 2 | -4.90617% | [-5.48206, -4.31364]% |
| retry, canonical, repeated | 16 | -27.11897% | [-27.37325, -26.86733]% |
| retry, canonical, repeated | 128 | -43.73888% | [-43.92416, -43.54794]% |
| retry, canonical, alternating | 2 | -5.13664% | [-5.85451, -4.43339]% |
| retry, canonical, alternating | 16 | -19.02690% | [-19.64979, -18.42216]% |
| retry, canonical, alternating | 128 | -24.95661% | [-26.75660, -23.72585]% |
| retry, pretty, repeated | 2 | +0.84671% | [+0.36866, +1.33967]% |
| retry, pretty, repeated | 16 | +0.01363% | [-0.33654, +0.35278]% |
| retry, pretty, repeated | 128 | +0.47581% | [-0.10206, +1.08454]% |
| retry, pretty, alternating | 2 | +0.45919% | [-0.11662, +1.04951]% |
| retry, pretty, alternating | 16 | +0.65772% | [+0.21649, +1.09762]% |
| retry, pretty, alternating | 128 | +0.95278% | [+0.36232, +1.62102]% |

Canonical 128-record retries improve 43.73888% with repeated checkpoints and 24.95661% with alternating checkpoints. Ordinary append intervals cross zero. Pretty JSON retains compatibility but some cases regress: repeated history 2 by 0.84671%, alternating history 16 by 0.65772%, and alternating history 128 by 0.95278%, with positive intervals. The remaining pretty comparisons are inconclusive. Scope-only pretty JSON is correctness-tested, not separately benchmarked. This optimization targets the canonical encoding written by Penelope; it does not accelerate every accepted representation.

All 24 final allocation profiles (both variants, four format/checkpoint patterns, three histories) remain exactly one allocation, 96 requested bytes, zero reallocations per retry, across 9,216 samples. This counts Rust allocator calls, not SQLite C allocations or total memory.

## Independent confirmation and remaining validation cost

A fresh 24-block three-variant confirmation used all six variant permutations four times and alternated ordinary/external profile order. Its 144 processes provide 504 latency observations. The historical pre-guard variant is `50856b001f81978daea5044947557a26429d9740`; it lacks the external-history validation fix and is a performance control only. The unchanged commit fixture and reused prior baseline executable hashes are recorded in provenance.

| Profile | Blob bytes | Final vs guarded baseline | Final vs pre-guard control |
|---|---:|---:|---:|
| ordinary | 0 | +1.17091% [-1.93140, +4.71902] | +1.90610% [-0.24674, +4.33382] |
| ordinary | 32 | -0.54738% [-3.98041, +2.97888] | +1.38637% [-0.80229, +4.45604] |
| ordinary | 4096 | +0.74397% [-1.05831, +2.80335] | +1.92493% [-0.27250, +4.15257] |
| ordinary | 1048576 | +0.12929% [-0.42344, +0.76068] | +0.17621% [-0.46774, +0.87414] |
| forced replay | 0 | -32.44107% [-33.23979, -31.75855] | +264.15427% [+252.86534, +271.13395] |
| forced replay | 32 | -32.88075% [-34.14877, -31.94021] | +264.51764% [+255.16096, +270.93326] |
| forced replay | 4096 | -31.73894% [-32.96403, -30.76148] | +251.72881% [+245.21718, +256.71397] |

Forced-replay append latency improves another 31.74–32.88% in confirmation, but remains 251.73–264.52% above the historical control (about 3.5–3.6 times its latency). That control skipped necessary validation; its behavior is not restored. Ordinary final/control comparisons all have intervals crossing zero. Neither comparison measures application-wide overhead.

## Rejected trials

The archived borrowed-RawValue prototype improved canonical repeated-history-128 latency by 16.86%, but introduced one extra 8-byte Rust allocation per record (129 allocations/1,120 requested bytes at history 128) and regressed pretty repeated-history-128 latency by 37.14%. It was rejected and its serde_json feature change removed. Borrowing RawValue avoids a payload copy but does not imply allocation-free parsing; see the [official RawValue documentation](https://docs.rs/serde_json/latest/serde_json/value/struct.RawValue.html).

An intermediate eager fixed-buffer trial improved canonical repeated-history-128 latency by 44.10%, but short pretty histories regressed 1.98–2.38%. The final decoder lazily prepares scope bytes only for a canonical prefix. Each rejected/intermediate campaign preserves its separate 480-process raw data, source snapshots, executable hashes, and 19 statistics; campaigns are not pooled. Including final confirmation, the archive contains 1,584 measured processes and 78 latency statistics. Prototype qualification logs describe those prototypes, not the final code.

An exploratory perf run collected 109 samples without lost samples, including integer/sequence JSON parsing and SHA work. It helped select the experiment; its short mixed setup/retry run is not quantitative attribution of replay latency.

## Correctness and qualification

The unchanged public-read decoder is the differential oracle. Tests cover 189 directed cases and 189 wrong-checksum controls, 10,000 deterministic byte mutations with recomputed checksums, all 256 repeated digest-byte scope patterns, exact/over scratch limits, large fallback records, duplicate/unknown fields, wrong identity, escaped/reordered/sequence forms, nesting, and scratch reuse with held owned results. A negative control that skips scope-byte equality fails with an incorrectly accepted record, demonstrating that the differential check detects this identity-validation error.

Final stable workspace all-target/all-feature tests (excluding the separate fuzz crate), SQLite all-target tests on Rust 1.85, strict workspace Clippy, formatting, layer boundaries, and graphify update passed. SQLite has 27 unit, 51 process, and 38 task tests. Eighty additional isolated stable/MSRV test processes cover 200,000 mutated records, 3,780 directed records plus 3,780 bad-checksum controls, 5,120 scope-byte patterns, 500 public column fixtures, 960 external-cache damage fixtures, and 180 replay-memo fixtures with 720 rejection checks. Existing corruption tests require rejection before durable mutation and successful repair/reopen.

[Evidence and reproduction instructions](verification-data/2026-10-31-task-scope-decode/README.md) include compressed raw data, per-phase source snapshots, build/test logs, environment and hashes, standalone analysis scripts, and a verifier. The Downloads release plan was not copied into the repository.
