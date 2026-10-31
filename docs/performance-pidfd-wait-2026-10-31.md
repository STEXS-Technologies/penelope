# Pidfd fence wait latency — 2026-10-31

Live-worker fencing across two CPUs has a 43.57% lower median paired latency in
the final campaign: 2.329 ms to 1.313 ms. The other 15 metric intervals include
zero. This establishes a gain in that fencing profile, not an application-wide
overhead percentage or a reliable full-workflow improvement.

## Change and scope

The old fence checked pidfd readiness without waiting, then slept one millisecond
when the worker had not yet exited. The new wait blocks on that same pidfd for
the remaining deadline and returns when the kernel reports readiness. Interrupted
waits retry with the remaining budget. Identity checks, generation authority,
reaping and failure-before-confirmation remain in place; public API, wire formats,
dependencies and portable core are unchanged.

Baseline is f31a8520c445b22d2b481d70b8a5478abb188e02. Final source is c889898d16374b6e66e364ac2b5fc7a0cbf94119. The new fence benchmark uses
real protected static C workers and a local SQLite WAL/FULL generation authority.
It measures the public fence call, including registry locking and authority
begin/confirm writes. Setup, worker startup and post-fence checks occur outside
the timer. The live worker sleeps until killed; the exited-worker control is
observed exiting before timing. The repeated-fence control has already fenced
once before timing. Checks reject a remaining active entry or an old-generation
restart after each timed call.

The parent is pinned to CPU 0. One run leaves its children on inherited CPU 0;
another pins the live/repeated worker to CPU 1 before timing and verifies its
mask. The exited control still inherits CPU 0. The existing checkpoint/ACK/result
workflow benchmark also inherits CPU 0 and measures fresh and reused ports.
Recorded topology identifies CPUs 0 and 1; the two placement cases are distinct
conditions rather than pooled samples.

## Final confirmation

Twenty alternating baseline/candidate pairs followed one warmup per binary/case.
Each fence process measures 32 samples per profile; each workflow process measures
16 per profile, excluding its internal reused-port warmup. Table times are
medians of process medians. Changes are medians of the twenty paired ratios;
therefore they need not equal the ratio of the displayed time medians. Negative
means lower elapsed time. Intervals use 10,000 paired bootstrap resamples with
seed 3030. All sixteen metrics and individual ratios are retained.

| Profile | Baseline ms | Candidate ms | Paired change | Bootstrap interval |
| --- | ---: | ---: | ---: | ---: |
| Live worker, CPU 0 | 1.341 | 1.322 | -0.27% | [-3.30%, +1.47%] |
| Live worker, CPU 1 | 2.329 | 1.313 | -43.57% | [-45.38%, -42.60%] |
| Exited worker, ordinary run | 1.260 | 1.287 | +0.26% | [-2.15%, +2.89%] |
| Exited worker, affinity run | 1.257 | 1.255 | +1.17% | [-0.14%, +5.00%] |
| Repeated fence, ordinary run | 0.089 | 0.090 | -0.07% | [-3.10%, +1.88%] |
| Repeated fence, affinity run | 0.086 | 0.089 | +1.78% | [-0.64%, +5.40%] |
| Full workflow, fresh ports | 8.700 | 7.433 | -8.50% | [-25.61%, +12.45%] |
| Full workflow, reused ports | 6.753 | 6.382 | -7.33% | [-35.19%, +5.30%] |

The final run retains 126 benchmark process records and 9,450 protected-worker
fixtures including warmups. No own project build, test or graph update ran during
timing. Other host workloads, CPU frequency and OS cache state were uncontrolled;
fresh ports do not mean page-cache-cold execution. These are measurements of
this host/profile, not universal bounds. In particular the full-workflow intervals
are wide and do not establish a reliable change.

## Pilots and qualification

The initial five-pair, same-CPU pilot did not establish a live-fence gain
(+0.16%) and showed positive fresh/reused workflow-completion medians (+19.01%,
+8.88%). This motivated testing a separate-CPU worker. The second five-pair pilot
showed −44.33% for that live-worker case, −2.20% for the same-CPU case and
−0.51%/+3.05% for fresh/reused completion. Both complete campaigns remain in the
evidence; they are not pooled into the final result. Their simpler blocking-wait
candidate is preserved as a patch. The final source adds budgeted interruption
handling and received its own independent confirmation.

Full workspace/all-target/all-feature checks passed on Rust 1.99.0 and 1.85.0,
excluding fuzz, with RUST_TEST_THREADS=1. Each includes 21 runtime library and
57 integration tests, worker allocation checks, benchmarks and examples. Strict
workspace Clippy, formatting, layer checks and the AST graph update passed.
Rust 1.85 also executed the optional CPU-1 benchmark branch.

Four new tests cover real pidfd timeout/readiness (including zero remaining time)
and scripted interrupted/early-empty/permanent-error outcomes. Nine exact
lifecycle/wait tests ran 50 times per compiler: 900 isolated successful executions.
These include stale identity rejection, deadline-overflow retry, reaping and
startup interruption recovery. No new fuzz campaign was run for this kernel
wait/deadline change.

## Evidence and reproduction

[Evidence manifest](performance-data/2026-10-31-pidfd-wait/evidence.json) pins
sources, compiler versions, binary hashes, commands, both pilots, full final
stdout, all summaries and qualification logs. Executables are hashed, not archived.
Recompute every summary and validate retained hashes with:

```sh
python3 docs/performance-data/2026-10-31-pidfd-wait/verify.py
```

For a fresh comparison, build each source with wrappers cleared using
`cargo build -p penelope-process-runtime --bench fence --bench dispatch --profile bench`.
Copy the executables to target/fence-wait-baseline-{fence,dispatch} and
target/fence-wait-candidate-{fence,dispatch}, then run the retained compare.py with
`--output target/new-fence-comparison --pairs 20 --samples 32 --dispatch-samples 16`.
It requires available CPUs 0 and 1 for these recorded placements. The repeat.py
script accepts a fresh workspace log and an output JSONL path to reproduce the
nine-test repetition campaign.
