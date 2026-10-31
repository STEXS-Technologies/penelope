# Timer input buffer capacity, 2026-10-31

The private executor timer-input builder now reserves its complete identity preimage and hexadecimal ID buffers. It releases the preimage buffers after hashing. Timer input IDs, canonical payload bytes, public APIs and storage settings are unchanged. The helper moves from `services.rs` into a private sibling module so benchmarks and compatibility tests can include the exact production source without exposing a public API.

The measured builder removes six reallocations per call and 302–1,655 bytes of Rust allocation traffic across 16 profiles. Allocation counts remain 4 for inline process scopes and 6 for heap-backed scopes; remaining serialization growth produces 2–4 reallocations. Requested bytes measure allocation traffic, not retained/peak memory or SQLite allocations.

Ordinary-allocator latency point estimates improve by 1.30–12.21% across two campaigns. Nineteen of 32 unadjusted per-profile 95% intervals exclude zero in the favorable direction; thirteen are inconclusive, and none supports a regression. These are private builder measurements, not public timer delivery throughput, whole-workflow latency or a new application overhead percentage. Historical application workload results remain pinned to their measured versions.

## Method and results

Parent `00b79aaf84e59ce1a74fd2c6b75bd5dc099c0650` supplies the original builder. The baseline library uses that parent's `services.rs`; the benchmark includes an exact extraction of its helper with parent-module visibility. The candidate library and benchmark use the same new private helper file. Common harness/fixture sources and baseline/candidate source and executable hashes are retained. Both variants pass the same pinned-parent compatibility test before measurement.

Two campaigns use six balanced ABBA/BAAB process blocks each, sixteen profiles per process, 100 untimed warmups per profile and 10,000 timed calls per profile. All 768 timing rows and 7,680,000 timed constructions are retained. Returned input/payload construction and disposal are inside timing; fixture setup, checksum calculation and JSON reporting are outside. Identical checksum, input ID and payload length are required across every variant/campaign. Allocation probes are separate: two repeats per variant yield 64 observations.

The runs use Rust 1.99.0, Linux/Ryzen 7950X and affinity to the first available CPU. No other project builds, tests, graph jobs or analysis overlap timing. Frequency and unrelated host load are uncontrolled. Percentages are geometric ratios within each six-block campaign; deterministic 20,000-sample block bootstraps supply unadjusted intervals. Rust 1.85 correctness is checked separately; its performance is not measured.

| Profile (identifier lengths) | Campaign 1 change [95% interval] | Campaign 2 change [95% interval] |
| --- | --- | --- |
| scope-128-step-128-action-128 | -6.53% [-9.99%, -0.06%] | -10.32% [-10.55%, -10.10%] |
| scope-128-step-128-action-5 | -3.28% [-6.78%, +3.32%] | -7.01% [-7.47%, -6.48%] |
| scope-128-step-5-action-128 | -6.12% [-9.34%, +0.05%] | -9.73% [-10.33%, -9.02%] |
| scope-128-step-5-action-5 | -6.55% [-9.84%, -0.26%] | -9.98% [-10.44%, -9.45%] |
| scope-22-step-128-action-128 | -4.66% [-10.91%, +8.57%] | -10.84% [-11.47%, -10.13%] |
| scope-22-step-128-action-5 | -4.54% [-11.74%, +10.94%] | -11.61% [-11.93%, -11.23%] |
| scope-22-step-5-action-128 | -2.46% [-9.79%, +13.13%] | -9.44% [-9.96%, -8.93%] |
| scope-22-step-5-action-5 | -1.30% [-9.39%, +15.46%] | -8.32% [-9.14%, -7.56%] |
| scope-23-step-128-action-128 | -5.62% [-8.92%, +0.52%] | -8.54% [-9.14%, -7.80%] |
| scope-23-step-128-action-5 | -7.62% [-11.22%, -0.65%] | -11.23% [-11.68%, -10.89%] |
| scope-23-step-5-action-128 | -7.66% [-11.84%, +0.71%] | -12.21% [-13.03%, -11.49%] |
| scope-23-step-5-action-5 | -2.94% [-9.01%, +9.76%] | -8.85% [-9.45%, -8.29%] |
| scope-5-step-128-action-128 | -3.34% [-10.81%, +11.77%] | -9.62% [-10.04%, -9.21%] |
| scope-5-step-128-action-5 | -1.90% [-9.18%, +13.36%] | -9.13% [-9.65%, -8.58%] |
| scope-5-step-5-action-128 | -1.81% [-7.97%, +10.04%] | -7.28% [-7.64%, -6.85%] |
| scope-5-step-5-action-5 | -2.59% [-7.18%, +6.08%] | -6.94% [-7.70%, -6.29%] |

## Compatibility and qualification

The pinned-parent oracle matches exact `ProcessInput` values and canonical payload bytes in 10,192 varied/boundary cases, including zero/maximum logical times and attempts, inline/heap/maximum identifier lengths and payload digest extremes. Another 30,576 owner/token/expiry substitutions preserve each input/payload pair. These pure builder tests do not authorize delivery of expired leases; public drain validation still precedes construction. Existing process-runtime crash/reconstruction tests cover payload, acceptance, acknowledgement and decision boundaries with separate/shared stores.

A standalone allocation guard covers all 16 profiles, limits reallocations to four and allocation calls to six, and caps requested bytes per profile with 64 bytes of headroom. The old builder fails this guard. Additional negative controls that let lease metadata affect identity or change the identity prefix fail compatibility checks. Production source is restored in `finally` blocks before final qualification.

Stable all-feature workspace tests excluding the fuzz package, Rust 1.85 executor/process-runtime tests and executor all-target checks, strict workspace all-target/all-feature Clippy, formatting and layer checks pass. Graph output is refreshed locally and stays untracked.

[Raw observations, provenance, qualification logs and verifier](verification-data/2026-10-31-timer-input-capacity/) retain all results. `python3 docs/verification-data/2026-10-31-timer-input-capacity/verify.py` checks the complete measurement grid, recomputes the summary, verifies source/provenance hashes and validates qualification and negative-control evidence. Reproduction build/measurement scripts must be copied to an empty target directory and run from this repository's root; build/negative-control scripts temporarily replace production source and restore it before returning. Do not run other project jobs during those scripts or ordinary measurements.
