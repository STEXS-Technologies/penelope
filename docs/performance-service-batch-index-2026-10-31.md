# Service batch duplicate index, 2026-10-31

Executor `claim_outbox` and `drain_timers` now use a bounded borrowed action-ID index for batches above 16 leases. They retain the original typed-ID pair scan verbatim for smaller batches. The new index sorts a separate name/position array, preserving delivery order, and finds the earliest position sharing either an action ID or fencing token with any later lease. Each lease is still validated before reporting a conflict starting at that position, preserving competing timeout/error precedence. Entire timer batches still validate before any payload write, inbox acceptance or acknowledgement.

With immediate injected ports, the final release benchmark measures 22.63–82.12% lower public outbox-claim latency for 32/64/128-item batches, with favorable intervals in all 36 comparisons. Large timer drains have 27 favorable and nine inconclusive comparisons; point estimates range from −10.32% to +0.38%. No final profile has a supported regression. Public allocation/reallocation counts and requested bytes are identical in all 72 profiles and both repeats. These CPU-focused fixtures do not establish SQLite claim throughput, storage latency, end-to-end application overhead or a universal speedup.

## Algorithm and limits

For a valid 128-item batch, the original scan performs 8,128 pair comparisons that can each compare long action IDs. The new large-batch helper sorts borrowed action strings with their original positions and scans adjacent equal-name groups. Fencing tokens remain opaque equality-only values: their compact local copies still use a bounded pair scan. Total worst-case complexity therefore remains quadratic in token comparisons; this change removes repeated long-string comparisons, not all quadratic work. No public API, token traits, schemas or durability settings change.

The two temporary arrays contain 128 name/index pairs and 128 tokens: 4 KiB of element storage on this 64-bit host, plus compiler/sort stack overhead. They allocate no heap memory. This is a bounded stack tradeoff for large batches; the actual compiler frame size and a whole-runtime stack bound are not measured. The private helper has safe scan fallbacks for small or oversized slices. Public batch limits remain 128 and reject oversized adapter responses before this helper runs.

## Measurements

Parent `be823698205af8fdbbc8f855d95667375b0a12ff` supplies the original services. Baseline/candidate builds use identical harnesses and fixtures within each experiment. The actual public async services run through immediate authorization, clock, claim, payload, inbox and acknowledgement ports. Claims move prebuilt lease vectors from the fixture. Timer input encoding, receipt cloning/validation, delivery and lease disposal occur inside the timer-drain call. Outbox returned-vector verification and disposal occur after timing. No SQLite, worker queues, network or durable I/O is exercised.

Each experiment has two campaigns, six balanced ABBA/BAAB blocks per campaign and 72 profiles: batches 1/8/16/32/64/128, identifier lengths 9/128, ascending/reversed/deterministically shuffled order and both public paths. Every process warms each profile twice and times 32 calls, using a fresh prepared fixture per call. Preparation and result verification/checksums are outside each timed interval; start/stop and service execution are included. All 3,456 rows, 110,592 timed service calls and 288 allocation observations are retained per experiment. A separate allocation executable uses the same fixtures. Checksums cover exact returned outbox leases or accepted timer inputs and must match across every variant, block and repeat. Timer payload digests, acknowledgement tokens/order and report counts are checked too.

Timing uses ordinary allocation, Rust 1.99.0, Linux/Ryzen 7950X and affinity to the first available CPU. No other project builds, tests, graph jobs or analysis overlap timing. Host background load and frequency are uncontrolled. Six block log ratios per campaign yield geometric percentage estimates; deterministic 20,000-sample block bootstraps supply unadjusted per-profile 95% intervals. Tiny outbox cases are particularly sensitive to timer/host/code-layout effects. Performance on Rust 1.85 is not measured.

The table retains the range across both widths, all three orders and both campaigns. A negative change means less time. Exact intervals and marginal medians remain in the compressed summaries.

| Public path | Batch | Change range | Comparisons with favorable 95% interval |
| --- | ---: | --- | ---: |
| Outbox claim | 32 | -53.09% to -22.63% | 12/12 |
| Outbox claim | 64 | -70.93% to -50.75% | 12/12 |
| Outbox claim | 128 | -82.12% to -67.59% | 12/12 |
| Timer drain | 32 | -4.00% to +0.38% | 5/12 |
| Timer drain | 64 | -3.84% to -1.30% | 12/12 |
| Timer drain | 128 | -10.32% to -4.46% | 10/12 |

The 72 small-batch control comparisons have no supported regression; 31 have favorable intervals and 41 are inconclusive. Their scan is unchanged, so no new small-batch algorithmic gain is claimed.

## Rejected implementations

`initial/` preserves a per-lease duplicate-mode branch: its large batches improve, but 13 comparisons show supported regressions, including small controls. `generic/` preserves a generic validation loop for every size: it has 51 supported regressions. Both candidates are rejected. `final/` restores the original small-batch loop and isolates the index to larger batches. All three raw campaigns, source identities, runtime patches and differing common-source snapshots are retained. The final fixture also replaces a setup-only remainder with checked remainder for strict Clippy; this occurs outside timing in both variants. No previous result is pooled into the final estimate.

## Reliability and qualification

A direct legacy pair-scan oracle covers 35,664 valid/duplicate cases, including every action-only and token-only duplicate pair at maximum size, threshold boundaries, maximum-width identifiers and competing action/token clusters. Public tests run 864 competing timeout/duplicate cases across both services, sizes 16/17/32/128, both widths and all three orders. They verify the exact error and timer report, with zero payload/inbox/ack side effects on rejection. Successful cases check exact returned order and verified delivery. The standalone allocation guard checks 18 size/width profiles, including oversized safe fallback, with zero allocations/reallocations/requested bytes.

Six negative controls are rejected: omit action conflicts, omit token conflicts, reverse error precedence, validate only the first 16 leases, allocate a token vector and bypass timer lease validation in the public wiring. Source-mutating scripts restore files in `finally` blocks before qualification. Stable all-feature workspace tests excluding the fuzz package, Rust 1.85 executor/process-runtime tests and executor all-target checks, strict workspace Clippy, formatting and layer checks pass. Graph output is refreshed locally and remains untracked.

[All observations, source provenance, qualification and verifier](verification-data/2026-10-31-service-batch-index/) are retained. Run `python3 docs/verification-data/2026-10-31-service-batch-index/verify.py` to check archive/source hashes, reproduce every experiment's summary and inspect qualification/negative-control evidence. For reproduction, copy an experiment's build/measure/analyze scripts to an empty target directory and run from the pinned checkout's root. Build/negative-control scripts temporarily replace source; do not overlap other project jobs with them or ordinary timing. Earlier experiment scripts require their archived source versions; the final build uses the current pinned source.
