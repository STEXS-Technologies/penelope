# SQLite rejected-request cleanup containment — 2026-10-31

Fix: `14c4342e94094ab81fe87cbc7873b8424ad6aa49`. Baseline: `8e4e2254182282fb9ae79a52bd8b17cc8cede68a`.

## Finding and fix

The private generic `DatabaseWorker::run` returns `QuotaExceeded` for a full queue and `Unavailable` for a disconnected queue. However, `try_send` also returns ownership of the rejected callback. Dropping that callback destroys captured application values; an unwinding destructor could escape admission instead of returning the known error. The existing boundaries around accepted operations, wake callbacks and worker-thread cleanup do not cover rejection on the submitting thread.

The error mapper now extracts the rejected callback, destroys it through the existing `contain_panic` helper and returns the original admission error. Rejected callbacks remain unexecuted. Accepted execution, queue capacity, transactions, response publication and wake behavior are unchanged. Successful admission does not enter the new cleanup boundary. No schema or public API changes are required.

This hardens an internal generic contract; it does not assert that ordinary current public SQL request types contain arbitrary application destructors. The helper retains its existing exceptional behavior for a second panic during panic-payload destruction: the secondary payload is deliberately retained rather than recursively destroyed. This handles unwinding panics, not process aborts, a double panic during stack unwinding, or indefinitely blocking destructors.

## Correctness evidence

Two regressions cover full and disconnected queues, each with ordinary and nested panic-payload destruction. The full-queue fixture gates the active request with channels, queues a second request and rejects a third. It requires the rejected capture to drop exactly once, its callback body to remain unexecuted, both accepted SQLite writes to become visible, and subsequent worker use to succeed. The disconnected fixture constructs an already disconnected private channel and requires the normal typed `Unavailable` result. A caller observation boundary distinguishes a returned error from admission unwinding and safely handles old-code nested payloads.

Against the pinned baseline worker and final tests, all four rejection scenarios unwind: both new tests fail after executing both modes. With the fix, all four return their expected errors normally. The source-restoring counterfactual script reruns complete SQLite unit/process/task suites on stable and Rust 1.85.0 after restoring the candidate. Both pass: 31 unit, 51 process and 42 task tests.

Twenty isolated worker-suite processes, ten per compiler, each pass all 13 worker tests: 260 test executions, including 80 new rejection scenarios and 160 existing canceled-result scenarios. These also retain the earlier operation/wake-payload, waker reentrancy/clone/drop, registration reuse, queue-exhaustion and cancellation checks. In-memory writes here prove ordering and visibility, not crash durability by themselves.

Stable workspace all-target/all-feature tests excluding the separate fuzz crate, SQLite all-target/all-feature tests on Rust 1.85, strict workspace Clippy, formatting, layer boundaries and graphify update pass. The baseline's GitHub Actions run `41179574642` completed successfully, including bounded fuzzing and optional StateChronicle conformance; its archived result is baseline CI, not CI of the new fix. No fresh local fuzz or optional StateChronicle run is claimed.

## Normal-operation latency

Unchanged public `lease_claim` and `lease_batch` release benchmarks compare baseline and candidate worker.rs, pinned to CPU 0 on Ryzen 9 7950X, Linux 7.2.9, Rust 1.99. There were no concurrent Penelope builds/tests, graph updates or bootstrap analyses during measurement. Twenty-four alternating AB/BA pairs contain 96 isolated processes and 288 observations. Empty profiles measure 2,000 claims per observation; nonempty profiles measure 400 across eight fixtures after warmup. Setup is untimed. Existing benchmark assertions validate results, attempt progression, unique lease tokens and persisted counters.

Changes below are mean paired candidate/baseline percentages; negative means faster. Intervals use deterministic 20,000-resample percentile bootstraps. All pairs are retained. These serial successful-admission profiles do not measure rejection cleanup cost, panic latency, allocation counts or global Penelope overhead. Individual intervals are not simultaneous guarantees.

| Claim | Batch | Change | 95% interval |
|---|---:|---:|---:|
| empty outbox | 0 | +0.07323% | [-0.44264, +0.54906]% |
| empty timer | 0 | +0.29601% | [-0.25173, +0.81649]% |
| outbox | 1 | -0.47647% | [-1.06963, +0.15637]% |
| outbox | 16 | -0.15028% | [-0.58779, +0.25473]% |
| timer | 1 | -0.64196% | [-1.28872, +0.00289]% |
| timer | 16 | -0.41319% | [-0.96045, +0.17530]% |

All six intervals include zero: this campaign resolves neither a speedup nor a slowdown. This is a reliability fix, not an algorithmic speed improvement. Measured source/fixture hashes match the final code after source restoration.

[Evidence and reproduction](verification-data/2026-10-31-worker-rejected-drop/README.md) includes raw measurements, source snapshots, executable hashes, baseline failures, qualification logs and a standalone verifier. The release plans in Downloads were not copied into the repository.
