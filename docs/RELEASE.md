# Penelope 0.2.0 release

Penelope 0.2.0 adds recoverable task execution alongside the preserved saga
engine. Seven library crates share version `0.2.0`; internal path dependencies
carry the same registry requirements. The fuzz crate is never published.
This branch is a prepared candidate; no tag or registry upload is implied.

## Task protocol and security review

The source review covers public contracts, deterministic replay, supervision,
effect bridging, optional checkpoint evidence, and hostile input boundaries.
It is a repository engineering review, not an external security certification.

| Boundary | Review conclusion and evidence |
| --- | --- |
| Definition identity | Scope binds the whole definition fingerprint, including every permission and budget. A frozen fixture and mutations of all fourteen fields protect the encoding. |
| History | Independent task version 1 leaves saga v1 unchanged. Strict new Serde representations reject unknown/duplicate fields. Replay checks scope, generation, sequence, monotonic time and legal transitions atomically. |
| Commit and dispatch | Input and generation authorization commit before execution. Checkpoints/results become facts only after atomic blob/history commit. Lost or mismatched receipts poison the supervisor until reconstruction. |
| Interruption | Restoring a running history fences the old generation and records interruption. Stale/cross-scope observations never advance current state. Fencing cannot undo an already-issued remote effect. |
| External effects | Every recovery mode preserves ambiguous effects. Independent evidence authorizes completion or restart; unknown remains blocked. Task exit, checksum and transport acknowledgement cannot prove success. |
| Checkpoints and upgrades | Size/schema/content checks precede use. Corruption rejects recovery. Application-authorized migration records old/new references in a newly pinned history; the library never transforms bytes. |
| Resources and clocks | Admission checks precede hashing/polling. Attempts, history, checkpoint count and blob sizes are bounded. Saturating time arithmetic preserves timeout enforcement near overflow. Runtime adapters enforce CPU allowance and bounded polling. |
| Storage integrity | Journals must reject corrupt records before replay. Host tests detect valid-JSON corruption using a checksum. Content hashes and semantic replay do not authenticate maliciously rewritten history; deployment integrity/authentication is adapter-owned. |
| StateChronicle | Optional independently verified state evidence is correlated by scope, generation and checkpoint identity. The boundary does not implement proof verification or require StateChronicle. |
| Portability | Allocation-free `penelope-core` contracts, projection and synchronous ports build without std, including optional Serde. The supervisor's owning crate and existing saga facade remain hosted. |

The definition fingerprint is SHA-256 over `penelope.task-definition.v1\0`,
code bytes, checkpoint schema (u32), effect tag (u8), recovery tag (u8),
attempts (u32), timeout (u64), restart delay (u64), checkpoint byte limit (u32),
input/output/history/checkpoint-count limits (four u32 values), CPU allowance
(u64), and absolute deadline (u64), in that order. All integers are big-endian.
Effect tags are Pure=0 and ReconcileRequired=1; recovery tags are Beginning=0,
Checkpoint=1, Reconcile=2, Never=3. This encoding is independent of Serde.

Security regression suites cover malformed wire values, substituted policies,
forged scope/generation, reordered/corrupt history, checkpoint substitution,
premature fault claims, repeated crashes, quota exhaustion, regressed clocks,
tick overflow, failed fencing, lost receipts, and poisoned reconstruction.
Real subprocess-kill tests exercise four fsync/rename persistence/effect
boundaries. Eleven fuzz targets run with sanitizer instrumentation, alongside
seeded property/chaos tests. Saga compatibility fixtures remain active.

Fault detection depends on trusted runtime observations. Recovery depends on
declared contracts. Task logic, checkpoint meaning, migration authorization,
durability, authentication, CPU enforcement, isolation and authoritative external
evidence remain application responsibilities. No RTOS, MPU/PMP, scheduler or
hardware integration is part of this release.

## Release validation

Exact source revision, seeds, counts, coverage and check results are retained
in [the campaign artifact](../ci/recoverable-execution-campaign.json).
The Rust toolchain is pinned to 1.99.0; the library retains its tested 1.85 MSRV.
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
./scripts/check-no-secrets.sh
./scripts/publish-crates.sh
git diff --check
```

The default packaging script verifies unpacked archives using Cargo's temporary
workspace registry, without uploading them. This checks packaged sources and
internal dependency resolution before any publication.

## Tagging and publication

After explicitly authorizing release publication, review the clean revision,
changelog and campaign evidence, then create an annotated `v0.2.0` tag.
Publish dependency-first with `./scripts/publish-crates.sh --publish`.
The script derives order from Cargo metadata, waits for exact versions to
enter the crates.io index, skips already-published versions and verifies all
expected versions. Credentials remain outside the repository.

Consumer adapter conformance must be assessed per deployment: transactional
commit/CAS, durable retention, idempotent starts, old-generation fencing and
independently verified effect evidence. Process-kill simulation does not certify
power-loss behavior or a production storage/runtime implementation.
