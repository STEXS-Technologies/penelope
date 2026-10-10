# Penelope 0.2.0 — Recoverable Task Execution

Penelope 0.2.0 provides deterministic, runtime-independent task recovery
alongside its saga engine. Applications declare recovery permissions, supply
journal and runtime adapters, and define valid checkpoints and effect evidence.

The seven library crates share version `0.2.0`. The release preserves saga v1
records, linear and graph decisions, retries, compensation and reconciliation.
This branch is a release candidate; tagging and registry publication are
separate steps.

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
The supervisor's owning crate, saga facade and asynchronous ports remain hosted.

Penelope is agnostic to runtime, operating system and storage backend.
Applications own task correctness, checkpoint meaning, migration authorization,
journal durability and authentication, runtime fencing and CPU enforcement,
and authoritative external-effect evidence. Recovery does not provide
instruction-level continuation or guarantee arbitrary program correctness.

## Validation

The test suite includes saga compatibility fixtures, task protocol fixtures,
property tests, real-thread CAS/fencing interleavings, and subprocess kills at
four persistence/effect boundaries: before checkpoint persistence, after blob
persistence, after history commit, and after an external effect before
acknowledgement. Eleven sanitizer-instrumented fuzz targets exercise protocol
and recovery boundaries. The task history fuzzer checks that every accepted
running state can still record a timeout.

The Rust toolchain is pinned to 1.99.0, with a supported minimum of 1.85.
Run from the repository root:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo +1.85.0 test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo clippy --workspace --all-targets --all-features --locked --exclude penelope-fuzz -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --locked --exclude penelope-fuzz
cargo check -p penelope-core --no-default-features --target riscv32imafc-unknown-none-elf --locked
cargo check -p penelope-core --features serde --target riscv32imafc-unknown-none-elf --locked
./scripts/run_coverage_json.sh
./scripts/check_coverage_ratchet.sh
PENELOPE_CHAOS_ITERATIONS=3 PENELOPE_CHAOS_PROPTEST_CASES=10000 PENELOPE_CHAOS_FUZZ_RUNS=100000 ./scripts/run_pure_chaos_drill.sh
cargo audit
cargo deny check
./scripts/check-action-pins.sh
./scripts/check-layer-boundaries.sh
./scripts/test-layer-boundaries.sh
./scripts/check-no-secrets.sh
./scripts/publish-crates.sh
git diff --check
```

Deployment adapters need their own conformance checks for transactional
commit/CAS, durable retention, idempotent starts, persistent fencing and
independently verified effect evidence. Host process-kill tests do not certify
power-loss behavior or production adapter implementations.

## Packaging and publication

`./scripts/publish-crates.sh` verifies the seven unpacked library archives
using a fresh Cargo temporary registry for each run. It excludes the fuzz
crate and retains verified archives under the configured Cargo target
directory's `package` directory.

For an authorized release, validate the release revision and changelog, create
an annotated `v0.2.0` tag, then publish dependency-first with
`./scripts/publish-crates.sh --publish`. The script derives dependency order
from Cargo metadata, waits for exact versions in the crates.io index, skips
already-published versions and verifies all expected versions. Credentials
remain outside the repository.
