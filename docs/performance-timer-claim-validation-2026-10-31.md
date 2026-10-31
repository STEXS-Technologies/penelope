# Borrowed timer claim scope validation, 2026-10-31

`TimerLease::validate_for_claim` now compares the action's five scope fields directly with the request. It previously constructed an owned `ProcessScope` solely for equality, cloning four identifiers. This follows the existing outbox validator's comparison pattern and removes unnecessary copies without changing public APIs, serialized records, SQLite code or durability settings.

The ordinary-allocator microbenchmark measures 28.98–33.88% less validation time for valid inline-ID profiles and 69.50–74.43% less for valid heap-ID profiles. Heap-ID validation also removes four allocations: 92 requested bytes at the 23-byte boundary and 512 at maximum size. The candidate allocates zero bytes in every measured profile. These are validator measurements, not end-to-end workflow latency or a new overall application overhead percentage.

## Results

Ranges cover two campaigns of the same release binary pair. Times are marginal medians; percentage changes use paired geometric ratios.

| Scope identifier length | Baseline valid ns | Candidate valid ns | Less validation time |
| ---: | ---: | ---: | ---: |
| 5 bytes | 15.664–15.744 | 10.823–10.829 | 30.84–33.88% |
| 22 bytes | 15.684–15.923 | 11.166–11.166 | 28.98–30.22% |
| 23 bytes | 41.379–41.481 | 10.608–10.611 | 74.33–74.43% |
| 128 bytes | 43.014–43.091 | 13.208–13.232 | 69.50–69.87% |

All 32 profiles are retained: four identifier lengths and valid, first scope mismatch, last scope mismatch, wrong owner, not-yet-due, expired, wrong action kind and excessive claim limit. Campaigns have 25 and 27 comparisons with intervals entirely below zero, respectively; neither has an interval entirely above zero. Early wrong-kind/limit returns never needed a scope and already allocated nothing. Other inline profiles also already allocated nothing. For heap profiles that reach scope comparison, baseline makes four allocations with no reallocations; candidate makes none. Rejected-input timings are included in the raw data and summary, not represented as successful claims.

## Correctness and qualification

An independent pinned legacy implementation compares exact `Result<(), PortError>` values in 18,432 cases: four identifier lengths, all 32 combinations of scope-coordinate substitutions, same/different owner, due time and expiry before/at/after the request time, all four action kinds, and valid/excessive limits. It also checks all 32 benchmark profiles. Both baseline and candidate pass this corpus, preserving request validation precedence, malformed-timer errors, scope/owner/due checks and inclusive expiry. The candidate's standalone guard requires zero allocations/reallocations/requested bytes in all 32 profiles. Baseline fails at `23-valid`, with four allocations and 92 requested bytes.

Final stable all-feature workspace tests excluding the fuzz package and Rust 1.85 all-feature ports/SQLite tests pass. Workspace all-target/all-feature strict Clippy, Rust 1.85 ports all-target checks, formatting and layer checks pass. SQLite's existing public integration tests and the timer deferred-COMMIT regression cases run as part of these suites. Graph output is refreshed locally and stays outside the commit.

## Method and reproduction

Baseline parent is `4fd5916a3a40c300f9f3975d0492f69ef5fcde5d`. Separate baseline/candidate release binaries use the same harness; only ports implementation source is substituted during the build. The build driver restores source in a `finally` block. No other build, test or graph operation runs during source mutation.

Two campaigns each have six balanced ABBA/BAAB blocks. Forty-eight ordinary executable invocations produce 1,536 rows and 153,600,000 timed validations, with 100 untimed warmups and 100,000 timed calls per profile/invocation. Return values are black-boxed; fixture construction and result formatting stay outside timing. Separate counter executables run twice per variant, giving 128 allocation observations. Ordinary timing uses the system allocator without counters. Every observed result is checked against the declared profile's expected result.

Each comparison uses six paired block log ratios. Twenty thousand deterministic bootstrap draws give unadjusted per-profile 95% intervals. The two campaigns share binaries and are not independent rebuilds. CPU 0, Ryzen 7950X, Linux 7.2.9 and Rust 1.99.0; no project builds, tests, graph work, allocation probes or analysis overlap ordinary timing. Background load and frequency are uncontrolled. Near-zero or noisy error-path results cannot establish a universal non-regression bound.

[Archived evidence and verifier](verification-data/2026-10-31-timer-claim-borrowed/) retain compressed raw timing/allocation rows, complete summaries, source/binary hashes, build/measurement/analysis scripts, the runtime patch, baseline/candidate parity and allocation-guard logs, and qualification. Run `python3 docs/verification-data/2026-10-31-timer-claim-borrowed/verify.py` at this version to check identities, the pinned patch, guards, exact expected results and recomputed summary. Release binaries are recorded by hash rather than stored in Git.
