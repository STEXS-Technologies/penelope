# Borrowed SQLite claim scope checks, 2026-10-31

SQLite outbox and timer claims now compare decoded action scope fields directly against the requested scope. The previous comparison constructed an owned scope for each decoded row, copying four identifiers. The private helper retains all five scope coordinates and the original validation order. Public APIs, stored bytes, transactions, lease fencing and WAL/FULL durability are unchanged.

Public maximum-length claims remove four allocations and 512 requested bytes per returned record. At batch 128, median allocation counts fall from 1,935 to 1,423 for outbox and from 1,295 to 783 for timers, with 65,536 fewer requested bytes in either case. Short-scope allocation counts and reallocations remain unchanged. Public latency evidence is modest: long-scope batch-16 outbox claims improve by 0.72–0.87% in both campaigns; most intervals include zero. No broad claim-latency or application-overhead improvement is established.

## Public allocation results

Counts and requested bytes below are medians across sixteen observations per profile/variant. They include the public asynchronous call, caller/waker work, worker dispatch, returned lease vectors and the real SQLite transaction. They exclude fixture creation, result validation, persistence checks and disposal of the returned vector. Rust allocation counters do not measure SQLite's C allocations or resident memory.

The pre-port reference uses the timer validator before `6f39123`; baseline includes that optimization; candidate additionally changes SQLite row scope checks. These three variants independently show the earlier validator saving reaches public timer claims, and distinguish it from this change's saving.

| Kind | Scope ID length | Batch | Pre-port allocations | Baseline allocations | Candidate allocations | Baseline requested bytes | Candidate requested bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Outbox | 128 | 1 | 30 | 30 | 26 | 7,086 | 6,574 |
| Outbox | 128 | 16 | 255 | 255 | 191 | 50,952 | 42,760 |
| Outbox | 128 | 128 | 1,935 | 1,935 | 1,423 | 388,520 | 322,984 |
| Timer | 128 | 1 | 29 | 25 | 21 | 6,016 | 5,504 |
| Timer | 128 | 16 | 239 | 175 | 111 | 33,424 | 25,232 |
| Timer | 128 | 128 | 1,807 | 1,295 | 783 | 248,464 | 182,928 |

For batch-128 timers, the two optimizations together remove a median 1,024 allocations and 131,072 requested bytes relative to the pre-port reference. Individual large-batch observations vary by up to two allocations and 144 requested bytes within a variant. Minima, medians and maxima are all retained; savings above use medians and are not a claim that every sample has identical counts. The short-scope controls at length 9 have identical allocation counts/bytes across all three variants. Reallocation counts are unchanged for every profile.

## Timing and correctness

Two ordinary-allocator campaigns compare baseline with candidate using six balanced ABBA/BAAB blocks each. Forty-eight executable invocations measure twelve profiles (outbox/timer, scope length 9/128, batch 1/16/128), with two warmup claims and eight measured claims each. This gives 4,608 timed public claims returning 222,720 envelopes. The first warmup is a fresh claim; measured operations reclaim expired leases after deterministic eleven-unit logical-clock advances. They are not fresh-claim latency measurements.

Every return is validated against the exact fixture action, expected delivery attempt, scope, owner and expiry contract. Token sequences are checked independently. Output checksums must agree across all observations of each profile/sample, including the three allocation variants. After each profile, raw SQL verifies every persisted lease envelope, attempt, expiry and the next token; a new adapter must not reclaim those unexpired leases. Definitions, commits, timer scheduling, request preparation, validation, SQL checks and reopen are outside the timed/counted regions. Those operations retain normal durability.

Timing covers 24 profile/campaign comparisons. None has an interval entirely above zero. Two long-scope batch-16 outbox comparisons have supported improvements (-0.873% and -0.724%, with unadjusted 95% intervals [-1.207%, -0.502%] and [-1.318%, -0.195%]); two other isolated comparisons have intervals below zero. The other twenty intervals include zero. All observations are retained; the small changes do not establish a universal latency improvement or non-regression bound.

Each block averages log times across both process invocations per variant and their eight samples. Twenty thousand deterministic bootstrap draws resample six block ratios, rather than treating correlated samples as independent. Both campaigns share the same binary pair. CPU 0, Ryzen 7950X, Linux 7.2.9, Rust 1.99.0. No project builds, tests, graph work, counters or analysis overlap ordinary timing. Frequency, unrelated background load, filesystem behavior and worker teardown noise remain uncontrolled. Allocation runs occur separately, twice per variant, yielding 576 observations.

## Regression checks and reproduction

The new private comparison is checked against owned-scope equality for all 32 substitution masks at identifier lengths 5, 22, 23 and 128, plus the 32 timer-validator fixture profiles. Five negative controls omit each scope coordinate in turn; every mutant fails. Two other negative controls restore the original SQLite implementation and fail the public outbox/timer maximum-batch allocation budgets. The mutation driver restores source in a `finally` block; no other project jobs run during those controls.

The standalone budget guard verifies both maximum-scope 128-record public claim paths, their returned contents and reopened state. Budgets allow ten allocations and 1,024 requested bytes above observed candidate maxima, and eight reallocations. Baseline exceeds these bounds by hundreds of allocations. Clippy required returning explicit errors from this `Result`-returning guard; final stable/MSRV guard runs and all-target checks cover that adjustment. The production code, scope test and measured harnesses were unchanged by it.

Stable all-feature workspace tests excluding the fuzz package and Rust 1.85 SQLite tests pass, including 45 SQLite unit tests, 72 process integration tests and 45 task integration tests. Workspace all-target/all-feature strict Clippy, Rust 1.85 SQLite all-target checks, formatting and layer checks pass. Existing corruption, race, statement-failure and deferred-COMMIT tests remain part of these suites. Graph output is refreshed locally and stays outside the commit.

Run `cargo bench -p penelope-sqlite --bench lease_claim_profiles` for ordinary profiles or `--bench lease_claim_profile_allocations` for counters. `cargo test -p penelope-sqlite --test lease_claim_profile_allocations` runs the allocation guard. Baseline parent is `6f391231456d766bc3003c87a453ee4cba58c857`; the pre-port reference substitutes only ports implementation from `4fd5916a3a40c300f9f3975d0492f69ef5fcde5d` into that baseline. Candidate changes only SQLite `leases.rs` runtime behavior.

[Evidence and read-only verifier](verification-data/2026-10-31-lease-claim-scope/) retain compressed raw rows, complete summaries, source/binary hashes, build/measurement/analysis and negative-control drivers, qualification and the runtime patch. Run `python3 docs/verification-data/2026-10-31-lease-claim-scope/verify.py` at this version to verify source identity, pinned patch, complete observation grids/checksums, all summaries and qualification. Binaries are recorded by hash rather than stored in Git.
