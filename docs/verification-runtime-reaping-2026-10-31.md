# Reconstructed Linux worker reaping — 2026-10-31

Confirmed fencing now reaps a stopped worker when the current host is its
parent, including when the original runtime and its Child handle were dropped.
The original runtime can also finish fencing after another reaper has consumed
its child's exit status. This closes a process-resource leak and a retry failure.

## Reproduction and implementation

Baseline 0656b7c506c59d576b58bfffc86e98566c40b8f6 exhibits two failures:

| Scenario | Baseline | Fixed |
| --- | --- | --- |
| Drop original runtime, then reconstruct and fence in the same parent | Fence succeeds but the worker remains a zombie in /proc | Fence succeeds and the worker's process entry is gone |
| Another host reaper consumes the worker's exit status before the original runtime fences | Original runtime returns Runtime after Child::try_wait reports ECHILD | Fence and its retry succeed; the old generation still cannot start |

Both baseline tests failed as expected. The first fixture retains a pidfd and
cleans up its deliberately reproduced zombie before asserting, so a failed
regression test does not leave that child behind. All signaled/reaped processes
were created by the tests.

Source 815acc8 preserves the boot/start-time identity checks, pidfd signal and
termination-readiness proof. After readiness, waitid(P_PIDFD, WEXITED | WNOHANG)
reaps through the same pinned descriptor. It never waits for an arbitrary child
or reaps by a reusable numeric PID. ECHILD at this point means this host cannot
reap the already-stopped process; its parent handles reaping. Other errors or
an unexpected not-ready result remain failures before authority confirmation.

For an active Child handle, ECHILD is accepted only after the registry identity
for that exact child PID has been proved stopped. ECHILD alone does not authorize
a fence. The PID-reuse test still verifies that a deliberately stale start time
leaves the live child untouched, then checks that stopping the actual identity
also reaps it. The expired/unrepresentable fence-deadline retry test remains.

The public API, wire/durable formats, dependencies and portable core are
unchanged. The supported host profile now explicitly lists waitid(P_PIDFD).
Dropping a runtime still does not acknowledge a durable fence: reconstruction
and fencing remain required. A worker belonging to another parent is reaped by
that parent. This audit does not force actual kernel PID reuse.

## Qualification and reproduction

Full workspace tests passed on Rust 1.99.0 and Rust 1.85.0, all targets and all
features excluding fuzz, with RUST_TEST_THREADS=1. Each run includes 17 runtime
library and 57 runtime integration tests, protected workers, distinct-process
coordinator crash/recovery matrices, benchmark smoke execution and examples.
Strict workspace Clippy, formatting, layer checks and the AST graph update
passed. The predecessor's seven CI jobs also passed; its result is retained
separately and does not stand in for CI on the new source.

Five scenarios received 50 isolated repetitions per compiler: stale saved start
time, unrepresentable deadline followed by retry, dropped-runtime reconstruction,
external child reaping, and interrupted startup reconstruction. All 500 process
executions passed. Benchmark smoke output is not performance evidence; no speedup
or unchanged overhead is claimed. No new fuzz campaign was run for this OS
ownership boundary.

[Evidence](verification-data/2026-10-31-runtime-reaping/evidence.json) retains
source/toolchain/binary hashes, commands, both baseline failures, the baseline
fixture patch, full validation logs and all repeated-process stdout/stderr.
Apply the fixture patch to the baseline and run the two listed baseline commands
to reproduce the failures. At the fixed source, repeat qualification with:

```sh
TMPDIR="$PWD/target/qualification-temp" RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
```

The retained repeat.py reads a fresh workspace log to locate its runtime library
and integration binaries, then runs the five exact test names 50 times each.
