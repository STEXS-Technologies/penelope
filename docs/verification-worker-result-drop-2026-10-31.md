# SQLite canceled-result destructor containment — 2026-10-31

Fix: `58e237f91028ecebb167de2a41075bc7357443de`. Baseline: `a0a7a6c69b04cca7a1056ab5eecf65df16db40f2`.

## Finding and fix

A canceled response can leave the database thread owning the last reference to its completion and result. The result is destroyed when the accepted job finishes. Operation and wake callbacks already had panic boundaries, but job cleanup ran outside them. An unwinding result destructor could terminate the worker and strand queued accepted requests; a panicking panic-payload destructor could additionally abort the test process while the detached thread's panic result was disposed.

The receive loop now executes each complete job through the existing `contain_panic` helper. Its boundary includes captured-state/result cleanup. Queued jobs continue after the canceled result's unwinding panic. The existing handling of a panic during payload disposal is reused: its secondary payload is deliberately retained to avoid recursively running arbitrary panicking destructors. This exceptional-path retention does not occur for successful jobs.

The change preserves admission bounds, serial execution, transactions, result publication and wake behavior. Cancellation can still lose an ACK after a committed operation; callers must reconstruct before retrying. This protects unwinding cleanup executed on the worker thread; it is not a claim that arbitrary process-abort signals or application-thread destructors can be recovered.

## Reproduction and correctness

Two new regressions cover ordinary and nested destructor panics. Each tests open/closed admission and registered/unregistered wakers. Channels deterministically block the first operation, cancel its response and admit a second job before releasing the first. All eight cases require the result to drop exactly once, the second job to complete and observe both SQLite writes, and continued worker usability when admission remains open. These fixtures use in-memory SQLite and prove ordering/visibility, not crash durability by themselves.

Separate old-code controls use the final tests against the pinned baseline worker.rs. The ordinary case fails with `Err(TimedOut)` instead of `Ok(2)` for the queued response. The nested case aborts its isolated test process with SIGABRT and `thread result panicked on drop`. Core dumps are disabled for both controls. They stop at the first scenario in each old-code test; they do not independently execute all eight old cases. The control restores production source in `finally` and reruns all SQLite unit/process/task suites on stable and Rust 1.85; both pass.

Twenty additional isolated stable/MSRV worker-suite processes run all 11 worker tests (220 unit-test executions), including 160 new canceled-result scenarios. The earlier operation/wake-payload, waker reentrancy/clone/drop, registration reuse, queue-exhaustion and cancellation regressions also pass. Final stable workspace all-target/all-feature tests excluding the separate fuzz crate, SQLite all-target tests on Rust 1.85, strict workspace Clippy, formatting, layer boundaries and graphify update passed. SQLite has 29 unit, 51 process and 42 task tests.

## Normal-operation latency check

Unchanged public empty/nonempty outbox and timer claim benchmarks compare only baseline vs fixed worker.rs. Both thread and caller inherit CPU 0 affinity on Ryzen 9 7950X, Linux 7.2.9, Rust 1.99 release builds. There were no concurrent project builds/tests, graph updates or bootstrap analyses during measurement. The 24 alternating AB/BA pairs contain 96 isolated processes and 288 latency observations. Empty observations contain 2,000 claims; nonempty observations contain 400 measured claims across eight fixtures after per-fixture warmup. Setup is untimed. Existing benchmark assertions validate claim results, attempt progression, unique lease tokens and persisted counters.

Changes are mean paired candidate/baseline percentages with deterministic 20,000-resample percentile-bootstrap 95% intervals. Negative means faster. Every pair is retained; intervals are individual comparisons, not simultaneous guarantees. These operation-specific data do not estimate global Penelope slowdown or prove a universal overhead bound.

| Claim | Batch size | Change | 95% interval |
|---|---:|---:|---:|
| empty outbox | 0 | -0.25609% | [-1.08414, +0.45947]% |
| empty timer | 0 | -0.68660% | [-3.08404, +0.82870]% |
| outbox | 1 | -0.93083% | [-1.72134, -0.15138]% |
| outbox | 16 | +0.02082% | [-0.41517, +0.46296]% |
| timer | 1 | -1.39575% | [-2.10765, -0.66830]% |
| timer | 16 | -0.83770% | [-3.06764, +0.53215]% |

No profile has a statistically resolved slowdown in this campaign. Two batch-size-one point estimates are faster with negative intervals, but the containment change is not presented as an algorithmic speed optimization. Allocation counts were not instrumented in this campaign. Measured source/fixture hashes match the final production and test sources after counterexample restoration.

The prior baseline's GitHub Actions run `41178695457` completed successfully, including bounded fuzzing, optional StateChronicle conformance and stable/MSRV checks; its status is archived separately and is not final-code CI. Local qualification excludes the separate fuzz crate and does not claim a fresh optional StateChronicle run.

[Evidence and reproduction](verification-data/2026-10-31-worker-result-drop/README.md) includes raw observations, exact source snapshots, executable hashes, baseline failures, qualification logs, scripts and a standalone verifier. The release plan in Downloads was not copied into the repository.
