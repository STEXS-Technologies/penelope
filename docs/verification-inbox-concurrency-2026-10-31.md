# Concurrent inbox admission and process commits — 2026-10-31

Tests: `a34c789cb52c07c9e4054013ecbe9b698dfc4bb5`. Production baseline: `693223b6a88392c1af11a162c7c84e5055a45502`.

The audit confirmed that duplicate input admission still feeds the existing consumed-input and canonical-source guards in atomic commits. It also confirmed that process_heads has a unique process_key, pinning each tenant/process to one definition scope. Cross-definition competitors therefore exercise that constraint as well as input/source checks; seeded same-definition cases at the next frontier are needed to qualify the latter guards directly.

Two new integration tests provide this coverage without changing production Rust sources, schema, Cargo manifests or lockfile. This is additional reliability evidence for the existing implementation and recent admission optimization, not a runtime bug fix or a new speed result.

## Cases and method

| Race | Initial state | Expected public results |
|---|---|---|
| identical input admissions | empty | two successes, one original and one duplicate |
| admission / commit of the same input | empty | two successes, consistent duplicate flags |
| identical atomic commits | empty | two successes, one original and one duplicate |
| same input ID, conflicting immutable payload | empty | one success, one Conflict |
| commits sharing one input across two definition digests | empty | one success, one Conflict |
| commits sharing one source event with distinct inputs across two definition digests | empty | one success, one Conflict |
| next-frontier commit reusing a consumed input / duplicate admission of that input | publicly committed seed | rejected commit and successful duplicate admission |
| next-frontier commit using a new input but reusing the seed's source event / duplicate admission of seed input | publicly committed canonical-source seed | rejected commit and successful duplicate admission |

For each case, two separate reference databases run both possible operation orders through the public ports. Explicit expectations independently constrain original/duplicate receipts and Conflict results; the reference states are not accepted merely because the implementation produced them. Public commit receipts validate against their requests, and inbox receipts must preserve tenant/process/input identity.

Two independent stores/workers then open the same fresh or publicly seeded raced database. A separate connection holds BEGIN IMMEDIATE while each public future is polled once and must become Pending. Channels confirm that both requests are in flight before releasing the writer lock, without sleeps or assumed thread-start overlap. This does not assert both worker threads are simultaneously in SQLite's busy handler.

The complete pair of public results and exact snapshots of all ten process/lease-counter tables must match one legal serial reference. Both original and duplicate flags are part of that comparison. Reopening must allow unchanged duplicate retries of successful requests and unchanged Conflict retries of rejected requests. The schema and input/source guards may reject after partial transaction work; losing commits must leave no input, usage, head, outcome, action or receipt rows behind. Snapshots include process_sources, process_timers, process_payloads and lease_counter as well.

## Repetition and controls

Twenty isolated processes, ten stable and ten Rust 1.85.0, pass both tests and all eight cases: 40 test executions, 160 gated races and 320 admitted requests. They return 220 successes and 100 Conflicts, matching 320 independently populated serial reference states. Reopened retries return 220 duplicates and 100 Conflicts, always without state changes.

| Race | First matching model 0 | First matching model 1 |
|---|---:|---:|
| duplicate-admission | 5 | 15 |
| admission-commit | 0 | 20 |
| duplicate-commit | 14 | 6 |
| conflicting-admission | 10 | 10 |
| shared-input-across-definitions | 17 | 3 |
| shared-source-across-definitions | 16 | 4 |
| consumed-input-at-next-frontier | 20 | 0 |
| reused-source-at-next-frontier | 20 | 0 |

The table records the first matching serial model, not an independently observed SQLite execution order. Seeded rejection cases have identical result/state pairs in both serial orders, so their first-match index is always zero and gives no scheduling information. The admission/commit race matched commit-first in all 20 concurrent runs; admission-first was exercised in its serial references, not observed concurrently. No test requires both orders to occur in random races; both serial reference orders are always executed explicitly.

Two mutation controls demonstrate sensitivity in the seeded cases. Disabling the consumed-input guard lets the rejected next-frontier commit and duplicate admission both succeed. Suppressing source-event insertion specifically for the competing input (leaving the seed binding intact) likewise lets both operations succeed in the reused-source case. Both controls are caught by the gated race's expected success-count assertion. Each control stops at that first intentional failure and does not execute every faulty combination. Source restoration is guaranteed by `finally`, followed by a passing complete process suite. These are deliberately introduced faults, not defects claimed in the production baseline.

## Qualification and limits

Stable workspace all-target/all-feature tests excluding the fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting, layer boundaries and graphify update pass. Both compilers pass 33 SQLite unit, 59 process and 42 task tests. Final process checks and repeated regressions also run after test-helper lint cleanup. Production sources/manifests remain identical to the baseline after control restoration. Baseline GitHub Actions run `41184991311` completed successfully, including bounded fuzzing and optional StateChronicle conformance; that status applies to the production baseline before these added tests. No fresh local fuzz or optional StateChronicle conformance run is claimed. Debug benches included in all-target qualification are functional executions, not controlled timing trials.

[Evidence and reproduction](verification-data/2026-10-31-inbox-concurrency/README.md) includes test/runtime/schema snapshots and hashes, failed controls, all repeated logs, final qualification, summary generation and a standalone verifier. These eight public-port cases do not exhaust every interleaving, corruption repair or power-loss scenario. No new whole-application overhead percentage follows. Release plans in Downloads are not included.
