# Timer identity encoder audit — rejected runtime change

The proposed table-based hexadecimal encoder is **not shipped**. It consistently reduced timer CPU time, but outbox control regressions persisted under normal build settings. A function-alignment diagnostic substantially changed those controls. That evidence is insufficient to call the change an overall performance improvement or justify changing project build settings.

Production `timer_input.rs` and the executor manifest remain byte-for-byte equal to parent `9fa82f60c39288b3512c1f1ece35077fb9b3b5cc`. The retained changes only improve the existing service benchmarks: optional `--timers-only` and `--outbox-only` switches, and separate non-inlined fixture entry points for the two services. Invoking either benchmark without a switch still covers all 72 profiles.

## Candidate and compatibility checks

The rejected candidate replaced 32 `write!("{byte:02x}")` calls with a fixed 74-byte ASCII buffer, nibble lookup, and checked UTF-8 conversion. It preserved one 74-byte allocation for formatting. It changed no digest preimage, identity prefix, payload, port call, or delivery protocol.

Both variants passed the existing 10,192 exact legacy input/payload cases with 30,576 delivery-metadata substitutions and the 16-profile allocation guard. The candidate additionally passed 259 formatting cases covering every byte value, leading zeros, ascending/descending byte order, and alternating bytes. Every formatting case required exactly one allocation, zero reallocations, and 74 requested bytes. Focused checks also passed on Rust 1.85.0. Five deliberate mutations—nibble order, uppercase alphabet, byte order, leading zeros, and an extra allocation—were rejected in each of the first three experiments. These prototype sources and checks are archived rather than added to the shipped test suite.

## Measurements

Each experiment used two campaigns of six balanced ABBA/BAAB blocks. Every block ran each variant twice. The private builder used 16 profiles and 10,000 calls per observation. The public service used 72 profiles, batch sizes 1/8/16/32/64/128, identifier widths 9/128, three lease orders, and 32 calls per observation. Public probes use immediate in-memory ports; they do not measure SQLite durability or an application workload. Allocation observations ran separately after all ordinary timing.

Reported ranges are point estimates of candidate time relative to the parent, grouped across both campaigns. Negative means less elapsed time. A supported gain/regression means the unadjusted, deterministic 20,000-resample block-bootstrap 95% interval excludes zero. These are many comparisons, not a family-wise significance claim. Short control calls are sensitive to code layout and machine conditions.

| Experiment | Private builder range | Timer service range; gains/regressions | Outbox control range; gains/regressions |
| --- | --- | --- | --- |
| Shared fixture invocation | −26.21% to −14.10% | −23.50% to −3.04%; 68/0 | −7.70% to +11.40%; 31/17 |
| Separate fixture functions | −28.58% to −15.58% | −22.65% to −4.24%; 72/0 | −1.84% to +27.21%; 1/50 |
| Separate service processes | −26.76% to −15.27% | −24.34% to −10.07%; 72/0 | −8.21% to +7.71%; 6/25 |
| Separate processes, diagnostic 64-byte function alignment | −25.71% to −15.89% | −24.72% to −8.61%; 72/0 | −9.41% to +3.29%; 41/1 |

Every private-builder comparison was a supported gain (32 per experiment). Allocation and reallocation counts and requested bytes matched between variants for every profile in all four experiments. All observations, including unfavorable experiments, are retained separately; no experiments were pooled or discarded.

In the isolated normal-build binaries, `Fixture::invoke_outbox` had the same instruction sequence after normalizing addresses and RIP-relative relocations. Function alignment changed from an address ending in `c0` to one ending in `a0`. Building both variants with diagnostic `-C llvm-args=--align-all-functions=6` reduced supported outbox regressions from 25 to 1. This supports code placement as a contributor, but it neither proves that alignment is the sole cause nor qualifies ordinary project builds. The diagnostic flag is not added to project configuration.

The existing six-case application workload does not invoke timer delivery, so it cannot establish an application-level benefit from this candidate. No new overall slowdown percentage is inferred from this audit.

## Evidence and reproduction

[Archived evidence](verification-data/2026-10-31-timer-hex-audit/) contains four complete experiments: compressed raw timing/allocation observations, exact summaries, measurement metadata, baseline/candidate source and binary hashes, source snapshots, build/measurement/analysis scripts, focused test logs, and fault-injection logs. Executables are identified by hash rather than committed. Final qualification logs cover the retained benchmark changes, not the rejected runtime candidate. Release-binary smoke checks confirm that default and isolated invocations produce the same complete profile set, checksums, and allocation measurements, accept Cargo’s `--bench` argument, and reject unknown or repeated service selections. The final argument parser includes this Cargo compatibility refinement after the archived timing snapshots; it does not change the measured service operations.

Run:

```sh
python3 docs/verification-data/2026-10-31-timer-hex-audit/verify.py
```

The verifier checks archive integrity and source provenance, confirms the runtime restoration, validates qualification and fault logs, and independently reruns all eight analyzers in temporary directories to reproduce every summary exactly. It does not rebuild or modify production sources.

To repeat timing, use a disposable checkout of the pinned parent. Restore one experiment's `candidate-sources.tar.gz` into that checkout (these snapshots include the prototype manifest and test); copy that experiment's build and measurement scripts into a fresh empty output directory. Run `build.py`, then `measure.py`, then each analyzer with its `helper` or `public` directory argument. Do not overlap builds, tests, analysis, or allocation probes with ordinary timing. The aligned experiment sets its diagnostic compiler flag in its build script; the other three do not. Fresh measurements are not expected to reproduce exact historical timing values.

The useful outcome is an isolated benchmark harness and a reproducible rejection record. Further timer optimization must demonstrate an acceptable tradeoff under normal builds, with a workload that actually exercises timer delivery.
