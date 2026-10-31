# Shared SQLite quota admission across workers — 2026-10-31

Tests: `e40213ec54e0bc13378bd30a3819df22d73133ed`. Baseline: `d45b988a9ae9b2e264aacc9ef81a8d7f1812c08d`.

## Audit finding and scope

Inbox admission, atomic process commits, timer scheduling and payload insertion all charge process_usage through their existing immediate writer transactions. Existing tests covered single-operation quotas, suppressed writes and competing outbox claims, but did not directly qualify heterogeneous writes competing for a shared quota on independent workers.

Two integration tests now cover that gap. Production Rust sources and Cargo manifests/lockfile are unchanged from the baseline. This is additional reliability qualification, not a runtime bug fix or a new speed measurement. Existing performance optimizations remain intact.

The tests target normal public-port admission and transaction behavior. They do not reconstruct retained-byte usage after external metadata deletion/rewriting, validate every possible corruption, measure physical disk usage, exhaust all interleavings or independently prove power-loss recovery.

## Test method

For each case, both operations first run independently on reference databases with generous limits. This produces two accepted serial states and measures each operation's charge rather than copying the implementation's charging formula. The shared-byte budget fits the existing seed plus either individual charge, but not their sum. The global admission test sets max_scopes=1 for two different process identities.

Two stores with independent workers then open the same fresh database. An external connection holds BEGIN IMMEDIATE while each public future is polled once and must become Pending. Both submitting threads acknowledge admission over channels before the lock is released. This gates two in-flight requests without sleeps or an assumption that thread starts overlap. It does not assert that both worker threads have reached the SQLite busy handler at a particular instant.

Exactly one operation must succeed and one must return QuotaExceeded. Snapshots of all ten process/lease-counter tables must equal the winning independent serial state. The fixture also reopens each reference under the constrained limit and requires its competitor to reject without any row changes, checking both serial orders. Reopening the raced database must allow an unchanged duplicate retry of the winner and an unchanged quota rejection of the loser. The atomic-commit cases compare inbox, outcomes, actions, head and receipt rows as well as the usage counter; an unsuccessful transaction must leave none of its partial state.

| Limit | Competing writes | Initial state |
|---|---|---|
| shared process bytes | payload / payload across two definition digests | empty |
| shared process bytes | inbox / inbox | empty |
| shared process bytes | payload / inbox | empty |
| shared process bytes | payload / atomic commit | empty |
| shared process bytes | payload / timer schedule | publicly committed timer action |
| process count | payload / payload | empty, different process IDs |
| process count | inbox / inbox | empty, different process IDs |
| process count | payload / inbox | empty, different process IDs |
| process count | payload / atomic commit | empty, different process IDs |

## Results and controls

Twenty isolated processes, ten on stable Rust and ten on Rust 1.85.0, each pass both tests and all nine cases: 40 test executions and 180 gated races. They confirm 360 admitted requests, 180 winners and 180 race quota rejections. They additionally check 360 serial-model quota rejections, 180 reopened duplicate retries and 180 reopened loser rejections, always with exact state comparison. All cases are retained. Each case observed both possible winners across the repeated runs; the complete distribution is archived.

| Limit / case | Left wins | Right wins |
|---|---:|---:|
| bytes/inbox-inbox | 4 | 16 |
| bytes/payload-commit | 6 | 14 |
| bytes/payload-inbox | 14 | 6 |
| bytes/payload-payload-cross-definition | 11 | 9 |
| bytes/payload-timer | 10 | 10 |
| scopes/inbox-inbox | 7 | 13 |
| scopes/payload-commit | 4 | 16 |
| scopes/payload-inbox | 11 | 9 |
| scopes/payload-payload | 7 | 13 |

Two mutation controls separately disable the shared byte guard and the process_usage count guard in process.rs. Each deliberately admits both writes in its first gated payload case and fails the exactly-one-winner assertion. The controls stop at that first failure; they do not independently run every faulty combination. Source restoration is guaranteed by `finally`; the restored complete 54-test process suite passes. The controls show that these tests detect missing admission bounds, not that the baseline has those faults.

## Qualification and reproduction

Stable workspace all-target/all-feature tests excluding the separate fuzz crate, SQLite all-target/all-feature tests on Rust 1.85.0, strict workspace Clippy, formatting, layer boundaries and graphify update pass. Both compilers pass 33 SQLite unit, 54 process and 42 task tests. The runtime process.rs hash and all production source/manifests remain identical to the baseline after control restoration; the committed change contains only test coverage and evidence.

Baseline GitHub Actions run `41182491463` completed successfully, including bounded fuzzing and optional StateChronicle conformance. Its archived status applies to the baseline, not CI of the new tests. No fresh local fuzz or optional StateChronicle gate is claimed. Debug benchmarks executed as functional parts of the all-target suites, not controlled timing trials.

[Evidence and reproduction](verification-data/2026-10-31-concurrent-quota/README.md) includes sources, hashes, raw controls/repeated runs, qualification logs, summary generation and a standalone verifier. Release plans in Downloads were not copied into the repository.
