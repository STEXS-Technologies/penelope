# Penelope release contract — 0.4.0 development

Penelope provides deterministic, runtime-independent task recovery
alongside its saga engine. Applications declare recovery permissions, supply
journal and runtime adapters, and define valid checkpoints and effect evidence.

Released 0.3.0 adds optional SQLite persistence and a protected Linux
single-process runtime. The 0.4.0 workspace adds the independent optional
`penelope-timeline` crate; all ten library crates use version `0.4.0`. Saga v1
records, linear/graph decisions, task history v1 and fingerprints retain their
existing formats. The [changelog](../CHANGELOG.md) separates unreleased work from
released source-compatibility changes. Historical [0.3.0 qualification](qualification-0.3.0.md)
and [performance evidence](verification-2026-10-30.md) retain their original
revision scope. Tagging and publishing remain separate authorized release actions.

## Rewindable timeline contract

The [timeline architecture and actual baseline mapping](timeline-foundation.md)
define a separate logical-step/revision/branch protocol. `S_t` is complete state
after applying `I_t`; state correction at `t` replays `t+1..head`, while input
replacement at `k` restores `k-1` and replays `k..head`. The independent
[`penelope.timeline.v1` binary metadata format](timeline-wire-v1.md) is protected
by golden fixtures; it is not a complete durable state recovery image.

Pure core decisions are allocation-free `no_std`. `penelope-timeline` supplies
caller-owned scratch replay, exact-head fencing, validation, bounded immutable
history and effect intents. `LiveTimeline` publishes state/head/intents under one
exclusive owner; optional `SharedTimeline` uses one hosted lock. Arbitrary
consumer state is safe only when snapshot isolation and the whole declared
consistency domain are honored. Missing history, stale results, incompatible
scope/model/schema, exhausted capacity and invalid validation fail closed.

Authority observations and irreversible-effect finality are separate frontiers.
Replay performs no intentional irreversible I/O. Explicitly finalized eligible
intents cross an idempotent durable-admission boundary; uncertain external effects
still require the existing task/saga reconciliation policy. Finalized history
cannot be corrected across a new authority epoch without application recovery.

The [two independent integration samples](timeline-examples.md) demonstrate this
contract, including a concrete SQLite v0.3 inbox/outcome/outbox handoff with two
lost acknowledgements. [Property/fuzz evidence](timeline-verification.md) and
[operation-level performance evidence](timeline-performance.md) disclose their
models, scope and limitations. Consumer determinism, snapshot completeness,
authority trust and effect finality are application obligations. No persistent
timeline adapter, distributed atomic promotion, Bevy/ECS, transport or rollback
of already-issued effects is supplied.
The [0.4.0 branch audit](qualification-0.4.0.md) records current gate evidence and
any remaining qualification work; no tag or publication is implied.

## Recoverable execution

Task definitions specify code identity, checkpoint schema, effect contract,
recovery mode and execution budgets. Four recovery modes are available:

- `RestartFromBeginning` restarts from committed input.
- `RestartFromCheckpoint` resumes from a committed application checkpoint,
  or starts from input when no checkpoint exists.
- `ReconcileBeforeRestart` requires independent effect evidence before recovery.
- `NeverRestart` prohibits automatic restart.

A task with the `ReconcileRequired` effect contract requires reconciliation
after interruption in every recovery mode. Unknown evidence keeps recovery
blocked. Completion requires a committed result or independent reconciliation
evidence; a crash, timeout or transport acknowledgement cannot prove success.

`TaskProjection` validates history and selects deterministic transitions.
`TaskSupervisor` coordinates persistence, execution, checkpoint acknowledgement,
fencing and recovery through application-supplied ports. Replay reconstructs
decisions without executing task code or repeating effects.

## Execution contracts

Task tenant, task, definition and input identities have distinct fixed-size
newtypes. Execution generations, history sequences and monotonic timestamps
also have distinct types, preventing accidental substitution at runtime and
journal boundaries. Their transparent encodings preserve the task v1 wire format.

| Boundary | Contract |
| --- | --- |
| Definition identity | Execution scope binds the complete definition fingerprint, including recovery permissions and resource budgets. |
| History | Task records have an independent version discriminator. Replay validates scope, generation, sequence, monotonic time and legal transitions atomically. Task wire decoding rejects unknown and duplicate fields. |
| Commit and dispatch | Immutable input and generation authorization commit before execution. Checkpoints and results commit atomically with their history records. Ambiguous or mismatched receipts require reconstruction before further dispatch. |
| Generations | Restoring a running history fences the previous generation before recording interruption. Persistent fencing rejects delayed starts and stale results while preserving newer generations. Concurrent supervisors require journal CAS arbitration. |
| Checkpoints | Size, schema and content identity are validated before use. Workers pause until durable checkpoint acknowledgement or fencing. Applications define valid computation and effect boundaries. |
| Upgrades | Checkpoint migration requires application-authorized state transformation into a newly pinned history. Penelope records source and destination references without interpreting application state layouts. |
| Resources | Attempts, history, checkpoint count and blob sizes are bounded. Live generations retain history capacity for a fault or completion. Rejected live quota proposals fence execution and require reconstruction. |
| Time | Monotonic clocks govern deadlines and restart delays. Late non-fault observations trigger timeout handling. Saturating arithmetic preserves deadline enforcement near tick overflow. Runtime adapters enforce CPU budgets and bounded polling. |
| Storage | Journals provide atomic blob/history commit, scoped linearizable reads, integrity checking and durable retention. Content hashes identify bytes; they do not authenticate rewritten history. |
| Effects | Already-issued external effects may complete after fencing. Independent reconciliation determines whether recovery or completion is authorized. |
| StateChronicle | Optional verified checkpoint-state evidence is correlated by scope, generation and checkpoint identity. Proof verification belongs to the implementing adapter; StateChronicle is not required. |

The definition fingerprint is SHA-256 over `penelope.task-definition.v1\0`,
code bytes, checkpoint schema (u32), effect tag (u8), recovery tag (u8),
attempts (u32), timeout (u64), restart delay (u64), checkpoint byte limit (u32),
input/output/history/checkpoint-count limits (four u32 values), CPU allowance
(u64), and absolute deadline (u64), in that order. All integers are big-endian.
Effect tags are Pure=0 and ReconcileRequired=1; recovery tags are Beginning=0,
Checkpoint=1, Reconcile=2, Never=3. This encoding is independent of Serde.

## Portable core and application integration

`penelope-core` provides allocation-free task contracts, projection and
synchronous ports without `std` or `alloc`, including optional Serde support.
`TaskControl` uses caller-owned `TaskWorkspace` buffers and shares its transition
and commit policy with the hosted `TaskSupervisor` wrapper. Operation-specific
`advance_with_buffers` and `poll_with_buffer` calls borrow dispatch and proposal
storage separately. The full-workspace calls retain admission of all capacities
before launch; poll storage must fit the larger checkpoint/output declaration.
The hosted wrapper validates its const capacity during restore and supplies two
arrays only when dispatching and one for poll. `TaskDispatchWorkspace` lets
portable callers supply dispatch storage lazily, after the shared policy chooses
a start/restart and before its authorizing commit. Waiting, fault and recovery
decisions do not invoke the provider. Actual stack frames remain compiler- and
adapter-dependent. The owning wrapper,
saga facade and asynchronous ports remain hosted.

Penelope is agnostic to runtime, operating system and storage backend.
Applications own task correctness, checkpoint meaning, migration authorization,
journal durability and authentication, runtime fencing and CPU enforcement,
and authoritative external-effect evidence. Recovery does not provide
instruction-level continuation or guarantee arbitrary program correctness.

## Integration boundaries

| Area | Consumer responsibility and limit |
| --- | --- |
| StateChronicle compatibility | Existing profile history encodings remain protected by StateChronicle's compatibility contract. Its new custom-domain enum variants can require source updates to exhaustive Rust matches; old readers must upgrade before reading custom histories. |
| StateChronicle custom domains | Custom transitions do not imply transactions across multiple resources. Schema upgrades require explicit versioning and application-defined migration; existing state must not be reinterpreted silently. |
| StateChronicle portability | Its portable components support `no_std`; the hosted executor and services remain hosted. Optional checkpoint evidence does not turn either library into an embedded runtime. |
| Journal durability | Real adapters must provide atomic blob/history commit, integrity, linearizable sequence/generation arbitration and retention. Runtime adapters must persist fencing across reconstruction. |
| Task safety | Restarting computation cannot establish its logical correctness or make physical side effects safe. Applications define valid checkpoints and independently reconcile ambiguous effects. |
| Execution environment | Task isolation, CPU-budget enforcement and immediate safety responses require a trusted execution adapter. Penelope authorizes recovery independently of any operating system or hardware protection mechanism. |

StateChronicle's custom-domain recovery suite exercises explicit schema migration,
versioned resource transitions, rollback, lost commit acknowledgements, concurrent reordered
delivery and reconstruction. That suite belongs to StateChronicle and is
independent of Penelope's optional evidence-correlation contract.

The original 0.2.0 SQLite integration fixture commits blobs and history in one
transaction, kills a separate process before and after commit, and reopens the
database to recover and complete the task. Independent connections also test
CAS arbitration and corruption rejection. That fixture is distinct from the
new optional [`penelope-sqlite`](../crates/penelope-sqlite/README.md) package,
which provides task and process persistence. The optional
[`penelope-process-runtime`](../crates/penelope-process-runtime/README.md) package
provides the declared Linux worker profile. Neither is a dependency of the
default facade. Their deployment contracts and qualification limits apply;
process-kill evidence does not certify power-loss behavior.

## Validation

The test suite includes saga compatibility fixtures, task protocol fixtures,
property tests, real-thread CAS/fencing interleavings, and subprocess kills at
four persistence/effect boundaries: before checkpoint persistence, after blob
persistence, after history commit, and after an external effect before
acknowledgement. Twenty-two sanitizer-instrumented fuzz targets exercise protocol
and recovery boundaries. The task history fuzzer checks that every accepted
running state can still record a timeout.
The worker protocol target checks bounded start-packet decoding, full scope and
generation substitutions, truncated and mutated metadata, proposal limits, and
exact checkpoint acknowledgements. It uses structured valid packets as well as
arbitrary bytes; it does not execute workers or qualify the Linux sandbox.
The SQLite task authority target compares reservation/fence/termination operations
against an independent state model using a private WAL/FULL database and separate
connections. It checks schema substitutions, conflicting identities, duplicate
commits, reopen behavior, immutable input bytes and exact journal replay after
each operation. Its bounded operation sequences complement the subprocess crash
tests; they do not simulate power loss.
The SQLite process target checks a real bounded WAL/FULL database against a
separate model of input acceptance/consumption, committed outcomes, delivery
attempts, inclusive lease expiry and acknowledgement. It also checks immutable
commit retries, conflicting retries, forged owners, scoped pending-input
visibility, payload digest admission and database reopen behavior. Every input
runs a fixed operation suffix as well as its mutated sequence; this target does
not simulate coordinator crashes or power loss.
Portable controller failure tests retain independent access to the test journal
and runtime. They check interruption receipt ambiguity, read failures and clock
regression before mutation, invalid input/checkpoint reads before dispatch,
rejected reconciliation evidence, unknown results across replay, and unconfirmed
quota fences. These are injected adapter-contract tests, complementing the actual
SQLite and process-runtime suites.
The supervisor fuzzer combines runtime proposals with lost acknowledgements,
wrong receipts, storage exhaustion and reconstruction, checking durable replay
and rejection invariants after each operation. A 32-contender thread test checks
that journal arbitration permits one execution. Campaign iterations pass the
same explicit seed to property tests and every fuzz target for reproducibility.
The timed property runner gives each registered property its own duration,
rotates seeds between batches and saves per-test elapsed time and case counts.
The newtype target directly exercises every public newtype's accepted values,
wire decoding and serialization, including malformed input and arithmetic
boundaries. AST-based tests enforce explicit newtype fuzz invocations and keep
all library `lib.rs` and `mod.rs` files limited to declarations and exports.

The Rust toolchain is pinned to 1.99.0, with a supported minimum of 1.85.
CI explicitly selects each job's stable, 1.85.0 or nightly toolchain through
`RUSTUP_TOOLCHAIN`, so the local pin cannot override its installed target or
components. Stable checks follow the current stable channel.
The full campaign also requires Python 3.11 or newer, nightly Rust with
`cargo-fuzz`, `cargo-llvm-cov` with `llvm-tools-preview`, `cargo-audit`,
`cargo-deny`, and `gitleaks`. RISC-V is the bare-metal target selected for
the cross-target `no_std` checks below; Penelope has no RISC-V requirement.
Install it with `rustup target add riscv32imafc-unknown-none-elf` only when
running those checks. Other suitable bare-metal targets can validate the
same portable-core contract.
Run from the repository root:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo test --workspace --doc --all-features --locked --exclude penelope-fuzz
cargo +1.85.0 test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo clippy --workspace --all-targets --all-features --locked --exclude penelope-fuzz -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --locked --exclude penelope-fuzz
cargo check -p penelope-core --no-default-features --target riscv32imafc-unknown-none-elf --locked
cargo check -p penelope-core --features serde --target riscv32imafc-unknown-none-elf --locked
cargo check -p penelope-timeline --no-default-features --target riscv32imafc-unknown-none-elf --locked
cargo check -p penelope-timeline --features alloc --target riscv32imafc-unknown-none-elf --locked
cargo bench -p penelope-timeline --bench timeline_operations --locked
cargo bench -p penelope-timeline --bench timeline_allocations --locked
./scripts/run_coverage_json.sh
./scripts/check_coverage_ratchet.sh
PENELOPE_CHAOS_ITERATIONS=3 PENELOPE_CHAOS_PROPTEST_CASES=10000 PENELOPE_CHAOS_FUZZ_RUNS=100000 ./scripts/run_pure_chaos_drill.sh
python3 scripts/run_property_stress.py --seconds 300 --cases 10000 --jobs 3
cargo audit
cargo deny check
./scripts/check-action-pins.sh
./scripts/check-layer-boundaries.sh
./scripts/test-layer-boundaries.sh
./scripts/test-chaos-runners.sh
python3 -m unittest discover -s scripts/tests
./scripts/check-no-secrets.sh
./scripts/publish-crates.sh
git diff --check
```

The [2026-10-30 verification record](verification-2026-10-30.md) retains current
workspace/MSRV, package, supply-chain and coverage evidence with its limits.

Deployment adapters need their own conformance checks for transactional
commit/CAS, durable retention, idempotent starts, persistent fencing and
independently verified effect evidence. Host process-kill tests do not certify
power-loss behavior or production adapter implementations.

The [pinned saga/contract comparison](performance-decisions-2026-10-30.md)
records identical benchmark datasets at the v0.2.0 baseline and a feature-branch
revision, with raw samples and RSS/wall evidence. Its slower saga medians remain
unresolved; it does not close the performance release gate.

The [fixed-port RISC-V stack analysis](portable-task-stack-bound-2026-10-30.md)
provides a conservative bound for one explicit freestanding reference profile.
The [SQLite file-cache measurements](performance-sqlite-cache-2026-10-30.md)
verify database-page residency before paired cold/warm replays. Their scopes
and limitations are separate from the final saga timing review and its
explicit performance exceptions.

## Source compatibility review

The pre-1.0 transition to 0.3.0 intentionally extends two exhaustive
public enums. `ProcessInputKind` adds `TaskEffectResult`;
`ProcessAuthorizationOperation` adds `Dispatch`, `DriveTask`, `ReconcileEffect`,
`DeliverTimer` and `RelayTaskResult`. Consumers must update exhaustive matches
and authorization policies. Existing variant numeric values are pinned by
regression tests; new variants are appended. Serialized legacy variant names
and v1 record formats remain unchanged. This is not a patch-compatible API
extension; consumers must review the documented 0.3.0 source changes.

A strict patch-level `cargo-semver-checks 0.51.0` comparison against the pinned
0.2.0 baseline identified these additions and reported the moved
`penelope_executor::task::SupervisorError` enum as missing. The hosted path is
still a public re-export. An external integration-test crate explicitly imports
its type and variants, exhaustively matches it, checks its original traits and
uses both error conversions on stable and MSRV. This compiler evidence resolves
the path-removal report for that usage; the raw checker report remains a failing
patch-level check. The deliberate enum additions still require the planned
pre-1.0 breaking release. No checker finding is silently suppressed.

## Packaging and publication

`./scripts/publish-crates.sh` verifies the ten unpacked library archives
using a fresh Cargo temporary registry for each run. It excludes the fuzz
crate and retains verified archives under the configured Cargo target
directory's `package` directory.

For an authorized release, validate the latest `main` revision and its dated
changelog entry, then create and push an annotated tag matching the qualified
workspace version (for this development series, `v0.4.0` after qualification).
The release workflow verifies the tagged revision and package version, publishes the crates
dependency-first, and creates a GitHub release from the matching changelog
section. Missing, empty or duplicate version entries stop release verification.
Manual verification does not create a GitHub release; manual registry publishing
requires a release tag and the explicit publishing input.

The underlying `./scripts/publish-crates.sh --publish` derives dependency order
from Cargo metadata, waits for exact versions in the crates.io index, skips
already-published versions and verifies all expected versions. Credentials
remain outside the repository.

The optional SQLite adapter has a [current store baseline](performance-sqlite-2026-10-30.md)
with raw pinned-CPU measurements. The final performance review links subsequent
cache and integration evidence; the historical core baseline does not cover this adapter.
The [hosted supervisor comparison method](hosted-task-memory-method.md) provides
a calibrated before/after allocation and sampled-stack runner. The
[initial measurements](performance-hosted-memory-2026-10-30.md) retain the original
stack regression. The [borrowed-restore follow-up](performance-hosted-borrowed-restore-2026-10-30.md)
records its removal for that fixture; the static RISC-V report supplies its
separately scoped conservative call-chain bound.
The Linux runtime also has a [real-worker dispatch/host-ACK baseline](performance-worker-dispatch-2026-10-30.md)
with fresh and reused store/runtime profiles, raw samples and disclosed limits.

Optional upstream signed-event conformance runs in its own pinned temporary
workspace through `scripts/run-statechronicle-conformance.py`. It includes
signed canonical triggers through full linear/graph real-worker sagas in both
store layouts and both effect modes, plus 136 actual SIGKILL/recovery cases
including the canonical source transaction and binding boundaries. The effect
reconciliation authority remains a trusted fixture. The same optional suite
also gates a real worker checkpoint through actual upstream signed sparse-Merkle
state proof verification, with fixture publication and pinned signer/head trust.
Production upstream execution, head discovery and external-effect proofs remain
outside this evidence. See [the integration contract and limits](INTEGRATION.md#optional-signed-canonical-event-conformance).

Deployment lifecycle procedures are in the [reliable execution runbook](reliable-execution-operations.md), including restart, unknown effects, leases, backup/restore and fence failures.

For verified database file-page cold/warm replay, see the [SQLite cache measurement method](performance-sqlite-cache-method.md). Reopened connections alone do not establish cold caches.

The [final development performance review](performance-final-2026-10-30.md)
supersedes earlier open timing dispositions, retaining measured regressions and
the explicit reference-profile qualification decision. Historical reports retain
their original source revisions and do not imply universal performance bounds.

The [graph optimization follow-up](performance-graph-optimization-2026-10-30.md)
removes the original graph slowdown exception, with oracle parity tests and
post-optimization validation. It retains host-load limitations and positive
control medians; the improvement does not imply all workloads have zero overhead.
