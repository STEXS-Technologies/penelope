# Bounded saga fixture completion wait — 2026-10-31

Baseline: `838505a` (exact commit pins and source hashes accompany the evidence).

The optional StateChronicle job for preceding commit `14f2ea2` failed with a pure saga still `Running` where the shared fixture expected `Completed`. Rerunning the failed job at the same commit succeeded (run `41170064541`, attempt 2). The fixture originally allowed offsets `first_offset..45`: 44 polls for pure execution or 37 for reconciled execution, with 2 ms sleeps. That provides only 74–88 ms of scheduled sleep, plus poll/advance work, before an otherwise valid running worker fails the test. This is an attempt count, not a meaningful host wall-clock deadline.

The fixture now uses a 10-second monotonic wall-clock deadline. It preserves the original logical transitions and caps subsequent polling at logical offset 44. It continues to require `Completed`, a valid durable result and the existing saga composition/recovery assertions. A worker that remains incomplete fails with its actual status at the deadline. Production runtime, task budgets, watchdogs, journal behavior and all persistence policies are unchanged.

A new test-only worker mode waits 300 ms using the already-permitted nanosleep syscall before emitting its normal sum result. The regression extracts the original fixture loop without changing its behavior and reproduces `Running != Completed` in 0.13 seconds against that delayed real worker. The final helper completes, verifies output 6 through the journal, and asserts at least 250 ms elapsed, no logical tick beyond 44, and exactly generation 1. This demonstrates tolerance for completion latency beyond the old polling window. It does not prove the precise scheduling cause of the original CI incident or guarantee that all future CI failures are eliminated.

Twenty isolated final regression processes—ten stable 1.99.0 and ten MSRV 1.85.0—exercise the real delayed worker and its durable output checks. The seven optional signed-source/checkpoint/saga tests also pass against the pinned StateChronicle revision `ae3d94bc22c7835eaa3fdcd766e3c117929c12bb`, including the pure and effectful SIGKILL recovery tests. The script verifies the upstream revision and tracked-source cleanliness and checks its exact seven-test selection before running it. The new standalone delayed-worker regression is exercised separately; it is not part of that signed-test filter.

[Evidence metadata and logs](verification-data/2026-10-31-saga-completion-wait/metadata.json) preserve the failing reproduction, repeated final tests and qualification results. Reproduce the targeted test with:

```sh
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo test -p penelope-process-runtime --test runtime saga_completion_waits_for_a_delayed_real_worker -- --nocapture
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo +1.85.0 test -p penelope-process-runtime --test runtime saga_completion_waits_for_a_delayed_real_worker -- --nocapture
```

No application latency or overhead comparison was performed for this fixture change.

Local qualification passed: workspace all-target/all-feature tests excluding `penelope-fuzz`, runtime MSRV all-target/all-feature tests (29 unit and 67 runtime tests), strict workspace Clippy, formatting, layer boundaries, and graph update. The final isolated repeats cover the added elapsed-time, logical-tick and generation assertions. Broad benchmark output is smoke validation only.
