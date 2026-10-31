# SQLite response registration audit — 2026-10-31

At source revision 1ccebdc5e0552113c7e955ac53a86c990fad0bd8, response polling
clones and destroys executor wakers outside the completion mutex. Repeated
pending polls reuse a registered waker when it will wake the same task.

The preceding implementation dropped the old registration while holding the
mutex. An executor destructor that reenters completion handling can deadlock;
unwinding can poison the response. The retained old-source reproducer observes
the locked mutex during destruction and fails deterministically.

The new registration path releases the lock before cloning, rechecks completion
before publishing the cloned registration, and drops the previous registration
after unlocking. If completion arrives during cloning, it stays stored until
temporary executor callbacks have returned successfully. Callback panics still
propagate on the polling thread; the response remains unpoisoned and its ready
result remains available. The existing worker-side wake panic boundary remains.

## Measured polling cost

The public OutboxStore::claim benchmark deliberately blocks its trusted clock
on the SQLite worker, then repeatedly polls its pending future. Each profile
performs 3,000,000 polls. Construction, SQLite work, release and collection of the
final response are outside the measured phase. Two distinct Arc-backed wakers
let the second profile switch registrations on every poll.

The final comparison uses 20 alternating baseline/candidate process pairs,
one warmup per binary, CPU 0 and Rust 1.99's bench profile. Baseline production
code is pinned by benchmark-only revision f168bf7921a3cad05faf5a0d068eeff0d76fe3d1.
The candidate is pinned above. Host workloads, CPU frequency and OS caches were
uncontrolled. No agent builds or tests ran during the comparisons.

| Profile | Baseline median ns/poll | Candidate median ns/poll | Change of medians |
| --- | ---: | ---: | ---: |
| Same registered waker | 10.948 | 8.341 | −23.81% |
| Switch wakers every poll | 10.963 | 13.998 | +27.69% |

The corresponding paired median changes are −23.81% and +27.81%, with paired
bootstrap 95% intervals [−23.89%, −23.64%] and [+27.51%, +28.16%].
The intervals resample paired ratios; the table compares the two sample medians.
The changing-registration path pays for another lock acquisition so executor
callbacks can run without the completion mutex.

A preliminary 20-pair round measured −23.72%/+28.05%. Adding the final test cases
changed the rebuilt executable hash, so the final build was measured again.
Both rounds retain all 84 process records, including warmups. This measures
pending polling in a deliberately isolated workload. Application polling
frequency, SQLite transaction latency and global Penelope slowdown were not
measured.

## Verification and reproduction

New tests check cloning and destruction with a reentrant callback, unwinding
during clone or temporary destruction, completion arriving during cloning,
reuse without repeated cloning, and delivery only to the latest registered
waker. The real queue test covers retained and canceled responses.

- Workspace all-target/all-feature tests passed on stable and Rust 1.85 before
  the last test-only additions. The final SQLite suite was then rerun on both:
  13 library, 31 process and 18 task tests, plus both benchmark smoke executables.
- All seven worker tests passed 100 direct runs on each toolchain: 1,400 test
  executions, including 6,400 latest-registration queue scenarios.
- Final strict workspace Clippy, formatting and layer checks passed.
- The knowledge graph was updated after source changes.

[Evidence and hashes](performance-data/2026-10-31-sqlite-waker-registration/evidence.json)
retain raw process outputs, build/check/stress logs, complete summaries, source
hashes, binary hashes, the comparison runner and a separate summary verifier.
Decompress each round's process JSONL beside its metadata and summary, then run
the retained summary verifier on that directory. The candidate fixture adds
only a comment and a lint allowance to the frozen benchmark; its measured
workload is unchanged.

The test-only RawWaker bridge uses unsafe Arc ownership conversions to inject
executor callbacks; production SQLite code retains its unsafe-code prohibition.
The result does not revise historical full-release qualification, package
verification, application-overhead figures or the extended fuzz campaign.
