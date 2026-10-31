# Direct canonical action effect-key encoding, 2026-10-31

`ProcessAction::canonical_effect_key_bytes` produces the same bytes as `action.effect_key().canonical_bytes()` while borrowing the action's identifiers. Both entry points use one private encoder. SQLite action insertion, duplicate-commit verification, outbox claim/integrity checks and executor timer-input identity generation use the direct method. The owned `EffectKey` API remains available. Serialized keys, stored records, timer input identities, transactions and durability settings are unchanged.

In the retained experiment, direct action encoding takes 44.06–66.50% less time across every measured profile/kind/campaign comparison. Maximum-size standalone actions allocate once instead of six times, removing five identifier allocations and 640 requested bytes. Public long-scope outbox claims remove four allocations and 512 requested bytes per returned record, because their step identifiers remain short. Batch-128 median counts fall from 1,423 to 911, with 65,536 fewer requested bytes. These are scoped encoding and public claim results, not an overall Penelope overhead percentage.

## Retained measurements

Ranges below span four action kinds and both campaigns. Times are marginal medians; improvements use paired geometric ratios and need not match ratios of the displayed medians.

| Identifier profile | Reference action ns | Direct action ns | Less encoding time | Reference/direct allocations | Reference/direct requested bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Short, 9 bytes each | 77.395–144.577 | 40.775–66.005 | 49.44–57.46% | 1 / 1 | 124 / 124 |
| Inline boundary, 22 each | 72.901–137.038 | 39.989–61.367 | 44.06–54.86% | 1 / 1 | 189 / 189 |
| First heap size, 23 each | 59.274–85.043 | 20.250–28.552 | 65.04–66.39% | 6 / 1 | 309 / 194 |
| Maximum, 128 each | 58.440–86.809 | 20.287–30.970 | 64.12–66.50% | 6 / 1 | 1,359 / 719 |
| Mixed, 5/22/23/64/128 | 63.107–102.469 | 26.426–43.717 | 55.97–60.21% | 4 / 1 | 536 / 321 |

There are no reallocations in either standalone encoding path. The separately measured existing owned-key entry point is a control: 27 of its 40 retained comparisons have intervals below zero, thirteen include zero and none is entirely above zero. All 40 direct-action comparisons have intervals below zero.

The unchanged public SQLite claim fixture measures outbox/timer, scope lengths 9/128 and batches 1/16/128. Long-scope outbox median allocations change from 26 to 22 (batch 1), 191 to 127 (16), and 1,423 to 911 (128). Requested bytes change from 6,574 to 6,062, 42,760 to 34,568, and 322,984 to 257,448. Timer and short-scope allocation medians remain unchanged. Large-batch observations can vary by two allocations and 144 requested bytes; complete minima, medians and maxima are retained. These Rust allocation requests exclude SQLite C allocations and resident memory.

Eight of the 24 retained public timing comparisons have intervals below zero and none is entirely above zero. The repeated supported case is long-scope batch-128 outbox: -3.389% with [-5.371%, -1.990%] in campaign 0, and -5.875% with [-9.361%, -3.476%] in campaign 1. Other improvements occur in only one campaign, including timer controls; they do not establish a general improvement for those paths. The remaining sixteen intervals include zero. Public timing covers warmed expiry/reclaim operations, not fresh claims or entire workflows.

## Rejected first version

The first shared-helper version removed the intended allocations, but its existing owned-key maximum-size control regressed in both campaigns (approximately 6–9% paired changes). It was rejected. The retained helper is forced inline and writes the first four identifiers explicitly, preserving the encoder's hot path while sharing its implementation. Both complete experiments are archived, with separate runtime patches, source/binary hashes, samples and summaries. The rejected experiment's results are not pooled into the retained tables.

The first experiment has twelve supported owned-key control regressions, forty supported direct-action improvements, and one isolated supported public timer regression. The retained experiment has no supported regressions in any measured family. This is evidence within these profiles and host conditions, not a universal non-regression bound.

## Correctness and regression protection

The deterministic 10,005-case corpus compares direct action encoding, owned-key encoding and a pinned independent legacy emitter byte-for-byte, across varied identifier lengths, all kinds, digests and attempt values. Action-ID substitution must preserve the bytes. Another 160 cases explicitly cover attempts 0, 1, MAX-1 and MAX, both zero/full-byte payload digests, all four kinds and all five profiles. Each performs nine separate semantic-coordinate substitutions (1,440 mutations), proving every coordinate remains bound while action identity is excluded.

A standalone twenty-profile guard requires exactly one allocation, no reallocations, exact output bytes and exact requested output length. Both experiments' reference dispatch fails at the first heap profile with six allocations; the candidate passes. The public maximum-batch outbox guard is tightened to 923 allocations and 258,616 requested bytes, retaining ten allocations and 1,024 bytes above measured maxima; the timer budget is unchanged. Restoring baseline production sources fails this public budget. Additional negative controls zero the encoded payload or remove the stored outbox effect-key comparison; the parity corpus and existing damaged-index integration test reject these faults. Source mutations restore all originals in a `finally` block, with no overlapping project jobs.

Stable all-feature workspace tests excluding the fuzz package and Rust 1.85 domain/SQLite/executor tests pass. The SQLite suite retains 45 unit tests, 72 process integration tests and 45 task integration tests, including corruption, replay, race and deferred-COMMIT coverage. Final targeted guards on both toolchains, workspace all-target/all-feature strict Clippy, Rust 1.85 all-target checks for the changed crates, formatting and layer checks pass. Graph output is refreshed locally and remains outside the commit.

## Method and reproduction

Baseline parent is `7deff215827c90afdd0c9abda5431033590410fa`. Each experiment builds separate baseline/candidate release binaries. Public harnesses are identical. New domain harnesses/tests require one dispatch adaptation for the baseline, which lacks the new API: `.canonical_effect_key_bytes()` is replaced with `.effect_key().canonical_bytes()`. Both dispatch sources are hashed. The fixture, timing loop and checks stay the same. The build driver restores runtime and adapted harness sources in a `finally` block.

Each experiment has two campaigns of six balanced ABBA/BAAB blocks per family: 48 domain executable invocations and 48 public invocations. Domain output has 1,920 timing rows and 192,000,000 timed encodings (two paths, twenty profiles, 100 warmups and 100,000 timed encodings per path/profile). Public output has 4,608 timed claims returning 222,720 envelopes; each process has twelve profiles, two warmups and eight measured expired-lease reclaims per profile. Fixtures validate returned actions, owner/scope/expiry contracts, attempts, token sequences, persisted envelopes and token counter, and unexpired state after reopen. All output checksums agree across variants. Setup, request preparation, validation, persistence checks, reopen and returned-vector disposal are outside public timing/counting.

Counters run separately twice per variant, producing 160 domain and 384 public observations per experiment. Across both experiments: 384,000,000 timed encodings, 9,216 timed public claims, 445,440 returned envelopes and 1,088 allocation observations. No project builds, tests, graph work, counters or analysis overlap ordinary timing. CPU 0, Ryzen 7950X, Linux 7.2.9, Rust 1.99.0, default SQLite WAL/FULL. Frequency, background activity, filesystem behavior and worker teardown are uncontrolled. The observed timing ranges include host variability.

Each interval resamples six paired block log ratios 20,000 times with deterministic seeds, unadjusted per comparison. Public block values average both executable invocations and their eight samples per variant; samples within a process are not treated as independent blocks. Each experiment's campaigns share binaries.

Run `cargo bench -p penelope-domain --bench action_effect_key` for ordinary encoding or `--bench action_effect_allocations` for counters. Existing public probes are `cargo bench -p penelope-sqlite --bench lease_claim_profiles` and `--bench lease_claim_profile_allocations`. [Archived evidence and read-only verifier](verification-data/2026-10-31-action-effect-key/) retain both experiments and qualification. Run `python3 docs/verification-data/2026-10-31-action-effect-key/verify.py` at this version to check identities, both pinned runtime patches, dispatch adaptation, complete data grids/checksums and recomputed summaries. Binaries are identified by hash rather than stored in Git.
