# Owned outcome-only append admission, 2026-10-31

The SQLite `append_outcomes` adapter now compares scopes by reference and moves its freshly constructed, owned `AtomicProcessCommit` directly into the worker job. Previously it constructed a temporary owned scope for every outcome and forwarded the owned request through the borrowed public commit API, which validated and cloned it again. The candidate retains the explicit caller-pinned scope check, constructor validation and authoritative worker-side validation. SQL, transaction behavior, durable schema, public signatures and dependencies are unchanged.

On the measured maximum-width 128-outcome fixture, caller-thread admission allocations fall from 2,061 to 775, with 198,176 fewer cumulative requested bytes. Short scope/short ID fixtures remove two allocations: the redundant outcome Vec and nested public commit future. Long scopes and IDs also remove their temporary string copies and the redundant validation scope. A maximum-width batch with a wrong scope in its last row is rejected without any caller allocations after future construction; the baseline makes 512 temporary string allocations before returning the same typed error.

These are allocation measurements, not an application slowdown percentage. No end-to-end latency or throughput improvement is quantified here. Persisting the decision still requires the same SQLite worker and durable transaction. Requested bytes describe allocation traffic, not peak memory or RSS.

## Measurement boundary

Pinned baseline: `5a04a54f1141c351b9a5a3b9c2d9ce30200a2699`. Identical fixtures and probes are compiled against both runtime variants. Only `SqliteProcessStore::append_outcomes` changes in the measured runtime. Each variant runs in two processes with eight samples of each of eight fixtures: 256 total allocation observations. All 16 observations within each variant/fixture agree exactly.

Fixtures cover scope identifier lengths 9/128, outcome and causal-input identifier lengths 8/128, and batch lengths 1/128. A healthy append seeds each database outside instrumentation. For each sample, an independent connection holds `BEGIN IMMEDIATE` while a first poll admits an exact append retry and returns `Pending`. Thread-local counters count only caller-thread Rust allocations in that first poll; database-worker allocations are excluded. Creating the outer async-trait future, fixture construction, database setup, waker construction, releasing the write gate, completion, replay verification and destruction are outside instrumentation. The queued operation then completes successfully, and public replay must exactly match the fixture's scope, outcomes and continuation. Fixture checksums agree across variants and repeats.

The probe uses release Rust 1.99.0 on x86_64 Linux. TLS counters are isolated to benchmark/test executables and add no production code. No allocator instrumentation is used to make latency claims. Absolute byte counts and allocation budgets describe the measured toolchains and fixture, not every platform or allocator.

## Caller-thread allocation results

Reallocation counts are zero for both variants in every fixture.

| Scope length | Outcome/causal ID length | Outcomes | Baseline allocations | Candidate allocations | Removed allocations | Removed requested bytes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 9 | 8 | 1 | 5 | 3 | 2 | 296 |
| 9 | 8 | 128 | 5 | 3 | 2 | 33824 |
| 9 | 128 | 1 | 9 | 5 | 4 | 552 |
| 9 | 128 | 128 | 517 | 259 | 258 | 66592 |
| 128 | 8 | 1 | 25 | 11 | 14 | 1832 |
| 128 | 8 | 128 | 1549 | 519 | 1030 | 165408 |
| 128 | 128 | 1 | 29 | 13 | 16 | 2088 |
| 128 | 128 | 128 | 2061 | 775 | 1286 | 198176 |

## Correctness and sensitivity

The new integration tests reject substitutions of all five scope coordinates at both positions of a two-row batch across lengths 5/22/23/128: 40 corruptions. Every rejection preserves all ten process and lease-counter tables. Repaired append and exact retry after reopening must succeed with the expected replayed history.

Four additional cases cover empty/seeded frontiers and one/128-row batches. They check empty and oversized batches, sequence errors, duplicate IDs, and a SQL abort on the last outcome insert. Failed operations must preserve the exact prior snapshot; healthy and repaired operations must match an independently executed public `ProcessStore::commit` reference in every row of all ten tables. Reopened exact retries must not mutate state; conflicting payload retries must return `Conflict`; replay and continuation must match the expected history.

Four deterministic cancellation cases exercise the optimized append API with one/128 outcomes and cancellation before commit or after result publication. An external write lock holds the admitted append, and a live public replay queued behind it establishes completion after cancellation. The result is never observed. Exact state matches a public commit reference, and retry after a clean reconnect changes no rows. The test does not require canceled futures to wake their registrations and does not simulate a process crash or power loss.

The standalone allocation regression guard rejects the baseline's 512 scope copies. A second negative control retains borrowed scope validation but restores forwarding through the borrowed commit API: the healthy admission guard rejects its 1,549 allocations, versus 775 for the candidate. Its budget permits only six long identifiers per retained outcome plus bounded Vec/validation/queue bookkeeping. Both source-mutating scripts restore production bytes in `finally`; archived hashes verify the retained source.

Final source passes stable workspace unit/integration tests with all features excluding the fuzz package, Rust 1.85.0 SQLite unit/integration tests with all features, Rust 1.85.0 SQLite all-target/all-feature compilation, strict workspace all-target/all-feature Clippy excluding fuzz, formatting and layer checks. The SQLite suites contain 36 unit, 70 process and 45 task tests, plus the standalone allocation guard. Graphify was updated. The baseline and candidate also passed the same focused scope/transaction-equivalence tests before comparison.

## Reproduction and evidence

Run `cargo test -p penelope-sqlite --test process append_ --all-features` and `cargo test -p penelope-sqlite --test process_append_allocations --all-features` for the retained guards. The `process_append_allocations` benchmark is an allocation probe; it intentionally does not emit timings.

Run `python3 docs/verification-data/2026-10-31-process-append-owned/verify.py` from the repository root to check source/archive hashes, reconstruct the measured runtime patch in a temporary directory, verify qualification and both negative controls, and recompute the eight comparisons from all 256 observations. It changes no repository source. Source checks require this version of the checkout.

For fresh observations, run the archived `compare.py <fresh-output-directory>` from an idle checkout. It temporarily substitutes baseline/candidate runtime source, runs focused tests and the baseline allocation negative control, builds both release probes, restores source, then runs the copied executables. Do not overlap its temporary source changes with any other build, test or source operation. The optional `copy-negative-control.py <fresh-output-directory>` separately verifies the second-copy guard and also temporarily modifies/restores source. Measured binaries are not archived; hashes are retained.

[Raw observations, qualification and verifier](verification-data/2026-10-31-process-append-owned/).
