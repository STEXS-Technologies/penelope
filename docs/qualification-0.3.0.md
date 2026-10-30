# 0.3.0 qualification audit

The full release-qualification reference is
`495451fc427231ee916d32e02ab8ee617d130941` on `feature/reliable-execution`.
The candidate passed local checks, remote CI and the versioned verification-only
release workflow. The historical graph slowdown is removed by the verified optimization below.
Other disclosed timing tradeoffs and deployment limits remain part of the disposition.
No tag, registry publication or GitHub release is authorized by this audit.

The subsequent linear-engine optimization at `8a9e298` removes discarded terminal
projection clones and reserves successor-history capacity. Its [direct-task
comparison and scoped verification](performance-application-overhead-2026-10-30.md)
measure approximately 0.24–0.26 µs of added one-step in-memory engine time.
The ~61 µs synthetic task has 0.43% median overhead; the ~3.8 µs task has 6.39%.
These exclude persistence and worker isolation. Earlier qualification records
remain evidence for their pinned revisions; the follow-up documents its additional
stable/MSRV tests, equivalence properties and targeted fuzzing separately.
[All seven code CI jobs](https://github.com/STEXS-Technologies/penelope/actions/runs/41132374645)
also pass on `8a9e298`.

A further [identifier-storage optimization](performance-inline-identifiers-2026-10-30.md)
at `aa2a557` has its own fixed comparisons and seven-job CI. The short-ID empty-task
added-time median drops from 245.29 ns to 119.17 ns; the ~3.8 µs task's overhead
falls from 6.34% to 2.87%. Standalone DTO encoding remains 6.29% slower, and the
long-ID control shows a smaller benefit. These scoped results exclude durability
and isolated workers. The follow-up also retains the prior CI worker-test failure
and the test-only bounded-deadline correction at `69a749f`.

The [encoding and polling follow-up](performance-encoding-2026-10-31.md) at
`53734a1` reduces current-branch DTO encoding by 4.16% and short-ID JSON decoding
by 9.47%; long-ID decoding measures 4.03% slower. The older-source timing control
is too variable to establish exact parity. More importantly, a real queued-frame/
child-exit race found through the preceding CI failure is fixed by requiring EOF
before exit-based fault classification. New deterministic and real-worker tests,
200 repeated worker test invocations, stable/MSRV workspace suites and both
seven-job CI runs pass. Historical full-campaign/package records retain their
original revision scope; this is separately retained follow-up evidence.

The [worker-protocol follow-up](performance-worker-protocol-2026-10-31.md) at
`2e3a768` removes the scope-hash allocation and enforces pinned payload limits
before payload access. Isolated allocation checks pass on stable/MSRV, 10,000
scopes retain legacy wire digests, and 100,000 targeted fuzz iterations pass.
The 20-pair protocol comparison has sub-1% proposal-time gains; request timing
intervals include no change. It does not revise application-overhead figures.

The subsequent [lease-counter audit](verification-data/2026-10-31-lease-counter/metadata.json)
at `1dd288b` rejects an ignored durable token-counter update before granting an
outbox or timer lease. The fault-injection test reproduces false success on the
preceding source, then proves rollback and recovery with the fix. All 56 SQLite
tests pass on stable and Rust 1.85, and strict workspace Clippy passes. This
requires no schema change or extra database operation. A 10,000-iteration SQLite
process fuzz run (seed 7) and [all seven source CI jobs](https://github.com/STEXS-Technologies/penelope/actions/runs/41143329178)
pass.

The [SQLite wake-callback audit](verification-data/2026-10-31-sqlite-wake/metadata.json)
at `751a010` contains notification unwinding after result publication, preventing
a panicking executor waker from terminating the database thread and stranding
already accepted requests. The old-source reproduction times out; the fix keeps
committed results and drains queued writes. The expanded regression at `99d9a3d`
covers retained/canceled responses with admission open/closed. It passes 100 times
on each of stable and MSRV (800 worker scenarios), alongside full local workspace
checks and 57 SQLite tests per toolchain. This is an unwinding-notification
reliability change; no new timing or full-release qualification claim is made.
Both [production CI](https://github.com/STEXS-Technologies/penelope/actions/runs/41144251403)
and [expanded-regression CI](https://github.com/STEXS-Technologies/penelope/actions/runs/41144326670)
pass all seven jobs.

The [graph transition-key follow-up](performance-graph-keys-2026-10-31.md) at
`005ea60` replaces tree storage with bounded per-source outcome bits. It preserves
validation errors in 40,000 generated reference comparisons and exhaustive
source/outcome-key tests. Stable/MSRV workspace suites, 100,000 targeted fuzz
iterations and all seven source CI jobs pass. The fixed 40-pair confirmation
measures 11.9% fewer graph instructions and 13.68%/16.49% lower serial/parallel
graph time. The initial unfavorable controls and identical-source timing
variation are retained; this does not revise application-wide overhead figures.

The [linear-validation follow-up](performance-linear-validation-2026-10-31.md)
at `dd3ac8d` replaces quadratic duplicate checks for larger definitions with
bounded borrowed-ID sorting and strict-descent admission. Frozen-reference
properties cover 40,000 generated cases; allocation fixtures, stable/MSRV
workspace suites, 100,000 targeted fuzz iterations and all seven source CI jobs
pass. The fixed comparison measures 82.76–98.39% less validation time for its
128-step fixtures. The pilot regression, small-definition tradeoffs and mixed
existing-workload controls remain disclosed. No application-wide slowdown or
full historical release requalification claim is made.

## Evidence and disposition

| Area | Checked implementation and evidence | Disposition |
| --- | --- | --- |
| Protocol and source compatibility | Legacy executor fixtures have no diff against `edcc5b4`; current stable/MSRV suites pin task v1 records/fingerprints, saga replay and original discriminants. The external `supervisor_error_compat` integration test checks the preserved hosted error path, variants and conversions. [Compatibility review](RELEASE.md#source-compatibility-review) retains the failing strict patch-level report and intentional exhaustive-enum additions. | Existing wire behavior and entry points retained; documented pre-1.0 source changes require consumer review. No patch-compatibility claim. |
| Portable supervision | Hosted wrapper delegates to shared core control; `task_control`/`task_control_failures` exercise parity, poisoning, ordering and buffer admission. Allocation fixture uses a positive control and five zero-allocation paths. Current stable CI plus local MSRV no-default/Serde tests and both MSRV RISC-V checks passed. | Pass for declared portable profiles. |
| Durable SQLite task authority | Actual journal tests cover atomic blob/history commit, CAS losers, missing records/heads, rebuild, quotas, retirement, corruption, lock bounds and scoped start/fence admission. Seven actual migration SIGKILL cuts retain exact v1 bytes, blobs and fence tombstones. | Pass for single-host local-filesystem WAL/FULL profile; no power-loss certification. |
| Durable saga services | `process` tests cover accepted inputs, outcomes/actions, canonical-source dedup, replay payloads, scoped claims, expiry/renew/ACK and worker isolation. `process_fault_tests` kills children at six transaction boundaries. Timer/service tests verify durable acceptance before ACK and later decision consumption. | Pass; no cross-database transaction assumption. |
| Protected worker | Real static C workers exercise sealed image identity, exact committed start admission, seccomp escape probes, CPU/address-space limits, checkpoint ACK, malformed messages, registry corruption, stop failure and reconstruction. `two_supervisor_processes_admit_one_real_worker_and_replay_one_completion` checks independent supervisors. Bootstrap tests cover synchronous and asynchronous load failure. | Pass for documented Linux x86-64 single-process/thread profile. Failed or unproven fencing blocks successors. |
| Typed task integration | Full linear/graph pure and effectful tests use shared/separate databases and actual child kills. Scope/code/schema/input substitutions fail. Versioned output classification and independent evidence gate saga success. Optimized graph validation is checked against the frozen original validator with 10,000 cases per property on stable/MSRV and explicit bound/cycle/error cases. `completed_worker_relay_crossing_cancellation_cannot_resurrect_the_process` checks a real completed worker against a cancellation-winning CAS. | Pass; task completion alone is not effect-success proof. |
| Optional trusted evidence | Current local upstream suite and remote CI pass seven tests, including 136 signed-source SIGKILL cases and real signed checkpoint/state proof validation. Exact upstream revision is pinned by the runner. Its head/publication authority and application effect source are fixtures. | Pass for optional adapter contract; production authority/effect services remain application responsibilities. |
| Adversarial campaigns | Recorded three-iteration campaign exited zero with 48 completed 100,000-input target runs. The final versioned CI bounded-fuzz job passes; the optimized graph/linear target also passes three additional 100,000-input runs. Timed property campaigns and real adapter kills have separately retained evidence. | Pass with exact revision/scope disclosure; campaign is not a single-SHA deployment-chaos certification. |
| Performance and memory | [Original review](performance-final-2026-10-30.md) retains both 20-pair six-benchmark campaigns, self-control, counters, RSS and worker samples. The [optimization follow-up](performance-graph-optimization-2026-10-30.md) retains all three optimization stages and the fixed 40-pair confirmation. SQLite append quantiles/history-length replay and verified file-page cold/warm runs are separately linked. Hosted allocation/sample-stack evidence and versioned RISC-V static analysis cover their declared profiles. | Graph slowdown exception removed: optimized median time is 15.24% lower than the original baseline, with 17.73% fewer instructions. Other positive control medians remain disclosed. No universal timing or RAM guarantee. |
| Operations and security | [Runbook](reliable-execution-operations.md) covers admission/authentication, clocks, restart, unknowns, leases/timers, upgrades, retention, backup, fence repair, telemetry and tenant isolation. Malicious-worker tests, private ownership requirements, typed bounded errors, quota checks and secret scans cover the supported boundary. | Pass within declared trust model. Authenticated principals and independent remote-effect evidence come from the application. |
| Build and supply chain | Versioned local fmt, stable/MSRV all-target tests, docs, strict Clippy/rustdoc, portable checks, layer/negative tests, Python/runner/action-pin checks, audit/deny, secrets, whitespace and nine package archive verifications pass. Coverage is 94.4655% against unchanged 94.0759%. All seven versioned remote CI jobs pass. | Pass: local archives and all seven CI jobs; verification-only release job passed with both publication jobs skipped. |

[Development gate evidence](verification-data/2026-10-30-final-development-gates/metadata.json)
retains the completed extended campaign and pre-version checks.
[Versioned gate evidence](verification-data/2026-10-30-versioned-qualification/metadata.json)
retains command logs, corrected metadata failure, exact candidate and hashes for
all nine verified 0.3.0 archives. The versioned performance manifest retains the
current generated RISC-V profile, disassembly/frame accounting and static image
section inventory. Earlier reports remain historical evidence at their pinned
revisions; their open dispositions are superseded only where this audit cites
new completed checks or an explicit exception.

Optimization results: [seven-job CI](https://github.com/STEXS-Technologies/penelope/actions/runs/41120267832)
at `495451f` and [verification-only release workflow](https://github.com/STEXS-Technologies/penelope/actions/runs/41119542159)
at `edf1194`. The intervening change is test-only atomic status-marker publication;
measured library sources are identical. [Optimization evidence](verification-data/2026-10-30-graph-optimization/metadata.json)
retains full local checks, 200 marker-race stress repetitions, new fuzz runs,
coverage and all nine reverified package hashes. The dated 0.3.0 changelog section was also extracted locally
with `scripts/release-notes.py`; manual nonpublishing workflows intentionally
skip tag-only checks. No release tag or published artifact was created.

## Limits that remain part of the contract

SQLite and the runtime are optional and absent from the default facade's normal
dependency graph. The verified environment is a private single-host local
filesystem and a trusted Linux kernel with the documented facilities. It does
not cover distributed/network-filesystem fencing, arbitrary native workers,
remote effects issued before termination, device/power-loss certification or
application-specific correctness. A lease token, digest, admin decision or
worker exit cannot independently establish effect success.

The static stack bound applies to the retained fixed-port RISC-V build and
aligned entry, excluding external callers, interrupts and board memory mapping.
The measured hosted workload and zero-allocation fixtures cover their selected
capacities/operations. Consumers must qualify their own ports, workspaces,
clocks, classifiers, effect authority and deployment.
