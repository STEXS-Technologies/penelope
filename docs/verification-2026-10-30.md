# Reliable execution verification — 2026-10-30

Candidate `0f4080b` passed the full final local check sequence: stable and MSRV
workspace/all-target tests, doctests, strict Clippy/rustdoc, both RISC-V profiles,
layer and negative checks, runner/Python/action-pin checks, audit/deny, all nine
package archives, secrets and whitespace. Coverage is **94.3494%**, above the
unchanged **94.0759%** ratchet. The three-iteration extended campaign completed
all **48 target runs of 100,000 inputs** and exited zero. Its revision scope and
separate actual-process-kill test scope are retained in the
[final gate metadata](verification-data/2026-10-30-final-development-gates/metadata.json).

The [final performance review](performance-final-2026-10-30.md) records all six
benchmarks, the parallel self-control, sealed-worker dispatch and explicit
measured regression exceptions. Earlier open campaign/performance dispositions
below are historical snapshots superseded by these results. Prior remote CI,
coverage, extended-chaos and verification-only release workflow results retain
their exact revisions in [remote metadata](verification-data/2026-10-30-remote-qualification/metadata.json).
Workspace metadata now declares the `0.3.0` candidate. Versioned package/workflow
verification and the final requirement audit are still pending. These results alone
do not declare a qualified 0.3.0 release.

The Rust sources at `2223a31` passed the checks below on the local Linux host.
This records current development evidence, not a completed 0.3.0 qualification.
The subsequent `0350d87` change updates documentation and a shell comment only.
Workspace versions remain `0.2.0`; no release tag or publication was performed.

[Retained logs and metadata](verification-data/2026-10-30-release-audit/) include
compressed command output, hashes, exact toolchain versions, advisory database
revision, coverage totals, dependency-first package order and SHA-256 hashes for
all nine verified package archives. These checks ran concurrently with stress
campaigns; their timings are not performance measurements.

Passed commands:

```sh
cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo +1.85.0 test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo test --workspace --doc --all-features --locked --exclude penelope-fuzz
cargo clippy --workspace --all-targets --all-features --locked --exclude penelope-fuzz -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --locked --exclude penelope-fuzz
cargo check -p penelope-core --no-default-features --target riscv32imafc-unknown-none-elf --locked
cargo check -p penelope-core --features serde --target riscv32imafc-unknown-none-elf --locked
cargo audit
cargo deny check
./scripts/publish-crates.sh
./scripts/run_coverage_json.sh
./scripts/check_coverage_ratchet.sh
python3 scripts/run-statechronicle-conformance.py /home/ac/projects/statechronicle
```

Core tests also passed without default features, with and without Serde, on
Rust 1.99.0 and 1.85.0. Both RISC-V feature configurations passed on 1.85.0 too.
This includes the isolated zero-allocation fixture; it does not establish a
worst-case stack bound. The optional upstream suite passed all seven intended
tests, including the 136 signed-source SIGKILL cases and actual signed checkpoint
state verification. Its publication/head authority remains a fixture.

Coverage was 8,809 of 9,363 lines, **94.0831%**, above the unchanged 94.0759%
ratchet. This is workspace coverage; the optional temporary upstream proof suite
is outside that coverage run. The layer gate and its negative tests, action-pin
check, chaos-runner registration checks, 28 Python tests and secret scan passed.
The default facade's normal dependency tree excludes both optional adapters.

An additional real-runtime concurrency test restores two independent supervisor
processes from the same committed intent, then releases a dispatch barrier. One
commits Started(1), runs the protected worker and commits completion. The other
receives a journal CAS conflict and remains poisoned. Durable replay contains
exactly intent/start/completion at generation one and the expected result bytes;
restoration preserves that terminal projection. This passed on Rust 1.99.0 and
1.85.0, along with the full runtime all-target suite and strict Clippy. Retained
logs and the added test source hash are in the metadata.

The three timed properties subsequently passed, each for at least 300 seconds
with 10,000 cases per batch and rotating seeds. The retained summary records
1,387 retry-policy batches, 945 replay-convergence batches and 75 seeded
checkpoint/persistence-chaos batches. These are requested property-case counts,
not counts of actual process kills.

The three-iteration chaos campaign failed in iteration one when the SQLite task
authority fuzzer could not open a fresh database (`Unavailable`). The exact
input passed 100 fixed-input repetitions. A longer replay on `/tmp` failed on a
read, while strace also reported **Disk quota exceeded** writing its own log.
This establishes a host quota problem during reproduction, but does not prove
the original failure has no adapter cause. The original log, input and failed
reproduction are retained. The exact input subsequently passed 10,000 native repetitions with the sanitizer
enabled and an explicit temporary directory on the local filesystem (exit 0).
A separate traced replay completed all 10,000 input repetitions without a
monitored syscall failure, then exited 1 because LeakSanitizer rejects ptrace.
It is not counted as a clean sanitizer pass. The random chaos campaign has not
passed. Its local-filesystem restart was intentionally stopped with SIGTERM so
the measured hosted stack regression could be addressed before the next full run.
Existing [saga performance concerns](performance-parallel-followup-2026-10-30.md)
and [worst-case memory/stack qualification](performance-portable-memory-2026-10-30.md)
remain open. No coverage, correctness or package check closes those requirements.

A strict patch-level API comparison of all seven original packages against
`edcc5b4` found shifted legacy enum discriminants. New variants were moved to the
end of `ProcessInputKind` and `ProcessAuthorizationOperation`, and regression
tests now pin every original numeric value. Domain/ports tests passed on stable
and MSRV, and the workspace all-target suite and strict Clippy passed. The
follow-up API report no longer reports shifted discriminants. It still fails a
patch-level policy for the deliberate exhaustive-enum extensions and reports
the shared error re-export as missing. The original hosted error imports,
variant imports, exhaustive match, traits and conversions compile in an
external test crate on stable and MSRV. See the [source compatibility
review](RELEASE.md#source-compatibility-review). The failing raw API reports are
retained; they are not represented as a green compatibility gate.

The SQLite schema suite now kills a real child at seven migration boundaries:
before the transaction, after begin, after each v2/v3/v4 DDL stage, before commit
and after commit. Each cut preserves exact task records, input bytes and confirmed
fence tombstones. Recovery observes either the original v1 schema or committed
v4 schema, completes migration, replays the same task and rejects delayed launch
of the fenced generation. Hooks compile only in Unix unit-test builds. The
all-target SQLite suite passed on stable and MSRV, along with strict Clippy.
Coverage remained above the unchanged ratchet at 8,815/9,370 lines (94.0768%).

The [hosted comparison](performance-hosted-memory-2026-10-30.md) records zero
allocation fixture paths but a stack regression at both 4 KiB and 64 KiB wrapper
capacities. This is an open qualification issue, not an accepted exception.

The per-call dispatch/poll buffer change at `24c02fa` passed stable and MSRV
workspace all-target tests, strict workspace Clippy and rustdoc, portable core
checks with/without Serde, the RISC-V target checks, release allocator tests,
and actual upstream conformance (seven tests including 136 process-kill cases).
New buffer admission, deadline ordering and foreign-input rejection tests passed
on both toolchains. Coverage passed at 8,856/9,413 lines (94.0827%) against the
unchanged 94.0759% ratchet. [Retained logs](verification-data/2026-10-30-task-buffers/)
record these checks. These checks do not replace the outstanding full chaos
campaign or final candidate audit.

The [buffer performance follow-up](performance-hosted-buffer-followup-2026-10-30.md)
uses one million native lifecycles per invocation. Sampled stack decreased from
the previous candidate, but still exceeds baseline, and native wall time remains
higher. Memory/performance qualification stays open.

The lazy dispatch workspace candidate `b5cd836` passed full stable workspace
all-target tests, focused parity/failure tests on stable and MSRV, strict
core/executor all-target Clippy, strict core/executor rustdoc, both MSRV RISC-V
core profiles, and release portable allocation assertions. Coverage passed at
94.1070% against the unchanged 94.0759% gate. The new provider test verifies no
buffer request for empty/running waits, timeout faults or recovery planning;
undersized start storage leaves the projection unchanged and permits retry.
[Raw logs and totals](verification-data/2026-10-30-lazy-workspace/) retain the
scope of these checks. Performance comparison and the full final-candidate
qualification remain pending.

The borrowed-adapter reconstruction candidate `7d2a2b9` passed stable workspace
all-target tests, MSRV executor all-target tests, strict workspace Clippy,
release portable allocator tests and coverage (94.1133%, unchanged 94.0759%
ratchet). Replay/fence/receipt validation now runs through borrowed journal and
runtime ports before constructing the owning control. [Raw logs](verification-data/2026-10-30-borrowed-restore/)
retain the checks; final candidate performance and full chaos qualification are
still pending. The preceding [scoped-buffer comparison](performance-hosted-scoped-frames-2026-10-30.md)
removed the 64 KiB sampled stack regression but retained a 4 KiB regression.

The completed [borrowed-adapter comparison](performance-hosted-borrowed-restore-2026-10-30.md)
records sampled lifecycle stack below the baseline at both tested capacities:
66,064 versus 85,056 bytes at 4 KiB, and 188,944 versus 202,304 at 64 KiB.
Native marginal medians are lower, with substantial 64 KiB variation. All paths
retain zero measured allocation and expected recovery/ACK assertions. This
removes the previously observed hosted sampled-stack regression for this fixture;
it does not establish static worst-case bounds or universal timing parity.
The full three-iteration chaos campaign has restarted on this production code,
with the temporary directory explicitly on the local filesystem.

A real-worker cancellation-crossing test closes an uncovered fault-matrix case.
It completes and independently classifies the worker result, retaining a success
batch prepared against the earlier running-saga frontier. After a durable unknown
outcome escalates the saga, two independent SQLite connections race the operator
cancellation against that stale result relay. Cancellation commits; the stale
result conflicts and remains rejected after reopen. Replay contains only start,
unknown and cancellation, no successful-action/completed outcome, and no next
action. Both shared and separate task/process database layouts pass. The fixture
supplies an already-authorized manual decision; it does not test operator identity
verification. The focused test passed on stable/MSRV, strict runtime Clippy passed,
and the full runtime all-target suite passed. [Logs and source hash](verification-data/2026-10-30-completion-cancellation/)
retain the exact scope and checks.

At candidate `f945a2a`, the full MSRV workspace all-target suite, workspace
doctests, strict workspace rustdoc, both MSRV RISC-V portable profiles,
`cargo audit`, `cargo deny check`, and all nine package archive verifications
passed. [Logs and archive hashes](verification-data/2026-10-30-candidate-checks/)
retain this checkpoint. Package versions are still 0.2.0 development versions;
no crates were published and no release tag was created. The restarted chaos
campaign remains in progress, and these checks do not replace its result or the
remaining performance and requirement audit.

The new SQLite file-cache harness passed an end-to-end smoke run: main-file
residency before cold/warm child launch was 0/39 versus 39/39 pages for 16 records,
0/72 versus 72/72 for 128, and 0/183 versus 183/183 for 512. The replay verifies
full projections and referenced bytes and rejects changed database hashes.
The default durable benchmark still executes, benchmark Clippy/MSRV checks pass,
and all 36 Python tests pass. [Smoke logs](verification-data/2026-10-30-cache-harness/)
are explicitly not qualification timing: the fuzz campaign was active.
A serialized run using the [cache method](performance-sqlite-cache-method.md)
remains required. The harness rejects partial residency and nonempty sidecars;
it never labels device or whole-system caches cold.

A [static-frame inventory](task-stack-usage-method.md) now complements sampled
stack measurements. The generated no_std fixed-port library retains repository
locked dependency versions and uses Rust 1.99.0, one codegen unit, aborting panics
and no red zone. The compiler records 89 function frames and detects the 64 KiB
positive control as 65,560 bytes. [Raw source, compiler usage, library and hashes](verification-data/2026-10-30-static-frame-inventory/)
retain this profile. All 39 Python tests and layer checks pass. Individual frames
are not complete call-chain bounds: precompiled runtime functions, ABI costs,
callbacks and possible recursion still require accounting. Worst-case stack
qualification stays open; this inventory does not silently substitute samples
or a maximum individual frame for that requirement.

The fixed-port freestanding RISC-V profile now has a complete conservative
[115,056-byte stack bound](portable-task-stack-bound-2026-10-30.md), including
all 42 reachable functions. Its scope is that exact linked reference binary,
not arbitrary adapters or hosted builds. Five paired SQLite replay runs also
completed with [verified cold/warm database file pages](performance-sqlite-cache-2026-10-30.md).
These close the corresponding reference-profile evidence gaps; they do not
close the parallel-saga performance concern or full adversarial campaign.

Remote CI at `2ed17b3` exposed clean-runner failures despite the local checks:
example scripts required unavailable ripgrep, and a nested StateChronicle
checkout caused Cargo workspace metadata inheritance to resolve against the
parent project. `c1279f3` uses standard grep for exact example-output assertions
and sibling checkout directories for the independent workspaces. The action-pin
check also uses standard grep and now rejects scan errors; tests verify that a
missing scanner input cannot silently pass. Both examples, all 52 Python tests
and all seven signed upstream conformance tests passed locally.

The [corrected remote CI run](https://github.com/STEXS-Technologies/penelope/actions/runs/41071761603)
passed all seven jobs: stable workspace/docs/Clippy/examples, MSRV, portable
core, actual pinned StateChronicle conformance, bounded fuzzing, supply-chain
verification and secret scanning. The failed run and corrected job snapshot
are retained with [local verification logs](verification-data/2026-10-30-ci-portability/metadata.json).
Remote bounded fuzzing uses 1,000 inputs per registered target and does not
replace the full local three-iteration campaign. Separate remote coverage and
extended-chaos runs were requested; their final results are not asserted here.

The local campaign previously observed live at session 12430 later stopped
without a recorded final status. Its retained partial log cannot count as a
pass. A new detached campaign records its exit status explicitly and uses the
same three iterations, 10,000 property cases and 100,000 fuzz inputs per target
on the local temporary filesystem. Final core/saga measurements are queued
strictly after its recorded successful completion, without concurrent local
fault testing. No 0.3.0 release acceptance, version bump, tag or publication
is claimed by these updates.

Remote extended chaos at `c1279f3` completed three iterations, each with 2,000
property cases and 10,000 fuzz inputs per registered target. The final log records
`pure chaos drill passed (3 repeated iterations)`. This is independent evidence
for that library-model workload; the larger local campaign still must finish.
Remote coverage at the executable-boundary correction `50fec4e` covered
8,933 of 9,468 library lines (94.3494%). The unchanged ratchet passed again when
checked locally against the downloaded report. The verification-only release
workflow at `c95a803` built and verified all nine development package archives;
its crates.io publication and GitHub release jobs were explicitly skipped.

An additional real-kernel startup test uses a recognizable static ELF with no
loadable segment. The initial local test assumed this always produced an
immediate startup error; CI on another kernel instead accepted the exec handoff
and faulted afterward. The corrected test covers both outcomes and two recovery
schedules. An immediate runtime error requires reconstruction; an accepted
handoff must fault if polled. Recovery without polling fences generation one
and records `SupervisorInterrupted`. Either path leaves exactly a fault after
intent/start, no completed output and no successor generation. Stable/MSRV and
strict Clippy checks pass locally. The
[corrected remote run](https://github.com/STEXS-Technologies/penelope/actions/runs/41077837306)
passes all seven CI jobs, including stable and MSRV. The original failing CI and corrected test diff are retained with the
[bootstrap timing evidence](verification-data/2026-10-30-bootstrap-timing/metadata.json).
This supplements the [executable-boundary regression](worker-executable-boundary-2026-10-30.md)
without equating launch admission with successful application execution.
