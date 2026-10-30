# 0.3.0 qualification audit

The source/metadata candidate is `16e3e1e3e3c7038aea4f3708cb9cc519fedaf664` on
`feature/reliable-execution`. Subsequent documentation/evidence commits do not
change the qualified Rust sources, dependency versions or build profiles.
The candidate passed local checks, remote CI and the versioned verification-only
release workflow. The performance exceptions below are part of this disposition.
No tag, registry publication or GitHub release is authorized by this audit.

## Evidence and disposition

| Area | Checked implementation and evidence | Disposition |
| --- | --- | --- |
| Protocol and source compatibility | Legacy executor fixtures have no diff against `edcc5b4`; current stable/MSRV suites pin task v1 records/fingerprints, saga replay and original discriminants. The external `supervisor_error_compat` integration test checks the preserved hosted error path, variants and conversions. [Compatibility review](RELEASE.md#source-compatibility-review) retains the failing strict patch-level report and intentional exhaustive-enum additions. | Existing wire behavior and entry points retained; documented pre-1.0 source changes require consumer review. No patch-compatibility claim. |
| Portable supervision | Hosted wrapper delegates to shared core control; `task_control`/`task_control_failures` exercise parity, poisoning, ordering and buffer admission. Allocation fixture uses a positive control and five zero-allocation paths. Current stable CI plus local MSRV no-default/Serde tests and both MSRV RISC-V checks passed. | Pass for declared portable profiles. |
| Durable SQLite task authority | Actual journal tests cover atomic blob/history commit, CAS losers, missing records/heads, rebuild, quotas, retirement, corruption, lock bounds and scoped start/fence admission. Seven actual migration SIGKILL cuts retain exact v1 bytes, blobs and fence tombstones. | Pass for single-host local-filesystem WAL/FULL profile; no power-loss certification. |
| Durable saga services | `process` tests cover accepted inputs, outcomes/actions, canonical-source dedup, replay payloads, scoped claims, expiry/renew/ACK and worker isolation. `process_fault_tests` kills children at six transaction boundaries. Timer/service tests verify durable acceptance before ACK and later decision consumption. | Pass; no cross-database transaction assumption. |
| Protected worker | Real static C workers exercise sealed image identity, exact committed start admission, seccomp escape probes, CPU/address-space limits, checkpoint ACK, malformed messages, registry corruption, stop failure and reconstruction. `two_supervisor_processes_admit_one_real_worker_and_replay_one_completion` checks independent supervisors. Bootstrap tests cover synchronous and asynchronous load failure. | Pass for documented Linux x86-64 single-process/thread profile. Failed or unproven fencing blocks successors. |
| Typed task integration | Full linear/graph pure and effectful tests use shared/separate databases and actual child kills. Scope/code/schema/input substitutions fail. Versioned output classification and independent evidence gate saga success. `completed_worker_relay_crossing_cancellation_cannot_resurrect_the_process` checks a real completed worker against a cancellation-winning CAS. | Pass; task completion alone is not effect-success proof. |
| Optional trusted evidence | Current local upstream suite and remote CI pass seven tests, including 136 signed-source SIGKILL cases and real signed checkpoint/state proof validation. Exact upstream revision is pinned by the runner. Its head/publication authority and application effect source are fixtures. | Pass for optional adapter contract; production authority/effect services remain application responsibilities. |
| Adversarial campaigns | Recorded three-iteration campaign exited zero with 48 completed 100,000-input target runs. The final versioned CI bounded-fuzz job passes. Timed property campaigns and real adapter kills have separately retained evidence. | Pass with exact revision/scope disclosure; campaign is not a single-SHA deployment-chaos certification. |
| Performance and memory | [Final review](performance-final-2026-10-30.md) retains both 20-pair six-benchmark campaigns, self-control, counters, RSS and worker samples. SQLite append quantiles/history-length replay and verified file-page cold/warm runs are separately linked. Hosted allocation/sample-stack evidence and versioned RISC-V static analysis cover their declared profiles. | Explicit graph timing exception: +2.95–3.10% marginal median, 28.53–29.90 ns per operation; smaller positive medians are also disclosed. No universal timing or RAM guarantee. |
| Operations and security | [Runbook](reliable-execution-operations.md) covers admission/authentication, clocks, restart, unknowns, leases/timers, upgrades, retention, backup, fence repair, telemetry and tenant isolation. Malicious-worker tests, private ownership requirements, typed bounded errors, quota checks and secret scans cover the supported boundary. | Pass within declared trust model. Authenticated principals and independent remote-effect evidence come from the application. |
| Build and supply chain | Versioned local fmt, stable/MSRV all-target tests, docs, strict Clippy/rustdoc, portable checks, layer/negative tests, Python/runner/action-pin checks, audit/deny, secrets, whitespace and nine package archive verifications pass. Coverage is 94.3494% against unchanged 94.0759%. All seven versioned remote CI jobs pass. | Pass: local archives and all seven CI jobs; verification-only release job passed with both publication jobs skipped. |

[Development gate evidence](verification-data/2026-10-30-final-development-gates/metadata.json)
retains the completed extended campaign and pre-version checks.
[Versioned gate evidence](verification-data/2026-10-30-versioned-qualification/metadata.json)
retains command logs, corrected metadata failure, exact candidate and hashes for
all nine verified 0.3.0 archives. The versioned performance manifest retains the
current generated RISC-V profile, disassembly/frame accounting and static image
section inventory. Earlier reports remain historical evidence at their pinned
revisions; their open dispositions are superseded only where this audit cites
new completed checks or an explicit exception.

Remote results: [seven-job CI](https://github.com/STEXS-Technologies/penelope/actions/runs/41113165133)
and [verification-only release workflow](https://github.com/STEXS-Technologies/penelope/actions/runs/41113175257),
both at `16e3e1e`. The dated 0.3.0 changelog section was also extracted locally
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
