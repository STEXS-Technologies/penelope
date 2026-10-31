# SQLite first-append retained-blob quota — 2026-10-31

Fix: `28e1db72baf12280e4dfcf8e9a476a19caf47f29`. Baseline: `4bb1f8394030600a306e86b6c5bffaae550da54d`.

## Finding and fix

A first task append could undercharge pre-existing scoped blobs and accept a write exceeding the configured scope quota. Reconstruction correctly summed all retained scoped bytes, but commit discarded that usage after validating a scope with no head. The final counter started at zero, then charged only newly inserted record/blob bytes. An existing input blob reused by the first intent was consequently not charged at all. Subsequent full replay could disagree with the newly persisted counter.

Commit now carries the validated usage alongside its projection. A cache hit uses the already checked head usage; reconstruction supplies its computed usage, including when the history is empty. The append adds only newly inserted bytes with checked arithmetic, enforces the configured bound and atomically writes the counter. Existing cached/nonempty-head validation, external-write detection, record/blob verification, bounds and transaction rollback are retained. No schema, API, dependency or durability change is required.

Concrete test examples (canonical first record: 1,048 bytes):

| Existing scoped data | Exact total after first append | Configured limit | Old result | Fixed result |
|---|---:|---:|---|---|
| 15-byte unrelated orphan; new 5-byte input | 1,068 | 1,067 | Accepted; counter omitted 15 bytes | Exhausted; rows unchanged |
| Existing 5-byte input reused by the intent | 1,053 | 1,052 | Accepted; counter omitted 5 bytes | Exhausted; rows unchanged |
| Same two cases at their exact totals | 1,068 / 1,053 | 1,068 / 1,053 | Counter undercharged | Accepted; exact counter survives reopen/rebuild |

Retained orphan rows can arise through external database changes or recovery/maintenance mistakes; normal task commit remains atomic and does not create these orphan states by itself. Existing malformed or undercharged heads still require explicit maintenance recovery: full validation rejects unexplained counters, and `rebuild_head` revalidates records/blobs and reconstructs derived metadata under its documented maintenance ownership requirement. This fix prevents new undercharging; it does not silently repair historical data.

## Regression evidence

The public-API regression covers 12 combinations: distinct orphan vs reused input, warm vs cold statement cache, and limits one byte below, exactly at and one byte above the required total. A 1,024-byte orphan in a different scope must not consume the tested quota. Each empty-history maintenance attempt is rejected without modifying durable rows. Over-quota first appends leave task heads, records, blobs and runtime generations unchanged. Raising the quota explicitly allows an exact retry, and read/rebuild/reopen preserve the precise raw-byte counter. Reused input is counted once.

Running the final regression against the pinned old tasks.rs fails because it returns `Ok(TaskRecord)` where `Err(Exhausted)` is required. That baseline control restores the fixed source in `finally`, then runs all SQLite unit, process and task suites on stable and Rust 1.85; both pass. The old control stops at the first failing case and is not evidence that all 12 old cases were independently measured.

Final stable workspace all-target/all-feature tests excluding the separate fuzz crate, SQLite all-target tests on Rust 1.85, strict workspace Clippy, formatting, layer boundaries and graphify update passed. SQLite has 27 unit, 51 process and 40 task tests. A test-variable naming cleanup satisfies strict Clippy; complete SQLite suites were rerun afterward on both compilers.

Sixty additional isolated stable/MSRV processes cover 240 quota fixtures with 80 atomic over-quota rejections, 960 external-history/cache damage fixtures, and 180 replay-memo fixtures with 720 rejection checks. These repeat the new quota suite and the existing cache/memo suites ten times per compiler. The preceding pushed commit's GitHub Actions run `41177453714` also completed successfully; it describes that prior code, including bounded fuzzing and optional conformance, and is not represented as fresh final-code CI.

## Rejected allocation optimization

Before the quota fix, an experiment returned the scoped blob-byte aggregate as a scalar subquery alongside each scanned record, retaining an explicit empty-history fallback. It reduced observed Rust allocations per retry from four/376 requested bytes to three/360 bytes across all 24 format/checkpoint/history profiles. However, canonical short-history retries regressed approximately 19%, and external-write-forced replay commits regressed 3–5%. The combined query was rejected. Production retains the prior statement-reuse design; the quota fix does not use the experimental SQL.

The experiment used Ryzen 9 7950X CPU 0 affinity, Linux 7.2.9 and Rust 1.99 release builds. There were no concurrent project builds/tests, graph updates or bootstrap analyses during measurement. Its 24 alternating AB/BA pairs contain 480 isolated processes, 19 latency comparisons and 9,216 Rust allocation samples. Retry observations contain 250 public already-reserved authorization retries; commit observations contain 128 appends. Histories 2/16/128 cover canonical/pretty JSON and repeated/alternating checkpoints. Setup and warmups are outside timing. Every observation is retained. Changes below are mean paired candidate/baseline percentages, with deterministic 20,000-resample percentile-bootstrap 95% intervals. Negative means faster.

| Rejected trial operation | History/blob bytes | Change | 95% interval |
|---|---:|---:|---:|
| commit, ordinary | 0 | -2.76791% | [-5.78184, -0.38867]% |
| commit, ordinary | 32 | -2.59722% | [-4.58527, -0.86936]% |
| commit, ordinary | 4096 | -0.94013% | [-2.59136, +0.70447]% |
| commit, ordinary | 1048576 | -0.04479% | [-0.55585, +0.40536]% |
| commit, forced replay | 0 | +3.02918% | [+1.32289, +4.73475]% |
| commit, forced replay | 32 | +3.37749% | [+2.07674, +4.60710]% |
| commit, forced replay | 4096 | +4.91310% | [+3.46437, +6.76997]% |
| retry, canonical, repeated | 2 | +19.15376% | [+17.79052, +20.71138]% |
| retry, canonical, repeated | 16 | +10.79002% | [+9.50737, +12.15928]% |
| retry, canonical, repeated | 128 | +2.29181% | [+1.19925, +3.28885]% |
| retry, canonical, alternating | 2 | +19.47760% | [+17.59380, +21.61588]% |
| retry, canonical, alternating | 16 | +8.75569% | [+6.91544, +10.56951]% |
| retry, canonical, alternating | 128 | +1.58911% | [+0.61537, +2.60880]% |
| retry, pretty, repeated | 2 | +14.51584% | [+12.75043, +16.51202]% |
| retry, pretty, repeated | 16 | +4.54914% | [+2.71173, +6.19047]% |
| retry, pretty, repeated | 128 | +0.87718% | [-0.41961, +2.25145]% |
| retry, pretty, alternating | 2 | +16.46658% | [+14.16540, +18.90798]% |
| retry, pretty, alternating | 16 | +5.90840% | [+4.47662, +7.62794]% |
| retry, pretty, alternating | 128 | +0.24990% | [-0.76491, +1.19944]% |

The rejected trial passed the pre-existing 27 unit and 39 task tests before the new quota regression was added. It was not fully qualified, and no query-plan or production-readiness claim is made for it. Its baseline also contains the quota bug found afterward. The benchmark fixture has ordinary valid histories and does not exercise that orphan edge case. These data support rejecting the SQL experiment, not a latency or allocation claim about the quota fix. No global Penelope slowdown percentage is derived from them.

[Evidence and reproduction](verification-data/2026-10-31-task-orphan-usage/README.md) preserves final qualification, baseline counterexample, production/test source snapshots, rejected trial sources, raw measurements, hashes and a verifier. The release plan in Downloads was not copied into the repository.
