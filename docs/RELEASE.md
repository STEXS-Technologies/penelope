# Penelope 0.5 candidate release contract

All ten library crates carry candidate version `0.5.0`. **This candidate is not
qualified or published.** The [plan](0.5/plan.md) defines acceptance, and the
[verification record](0.5/verification.md) identifies passing checks and open
chaos, performance and final artifact gates. The [changelog](../CHANGELOG.md)
remains unreleased. Tagging and publication require separate authorization.

## Machine and execution contract

`MemoryMachine` prepares and validates typed state transitions and bounded intents
before consuming publication. Optional hierarchy uses one active path and explicit
parent fallback. Local publication supplies neither durable acceptance nor external
authority. Start with the [canonical tutorial](getting-started.md) and
[API map](0.5/public-api.md); the [machine contract](0.5/machine.md) defines the limits.

The [SQLite journal and outbox](0.5/machine-storage.md) retain accepted
head/state/event/intents atomically. Native task/saga receivers verify source-bound
admission through their own protocols. Journal-before-dispatch, task generations,
saga compensation and unknown-outcome reconciliation keep their original owners.
The new `penelope.machine.v1` protocol does not rewrite saga/task/timeline v1 history.

## Rewindable timeline contract

The [timeline contract](timeline-foundation.md), [wire format](timeline-wire-v1.md)
and [examples](timeline-examples.md) define correction, captured context, bounded
history and coherent publication. The machine adapter reuses that engine.
Snapshots must isolate the complete consistency domain; replay performs no
intentional irreversible I/O. Authority observations and irreversible-effect
finality remain separate. Finalized intents require durable idempotent admission;
uncertain external effects still require reconciliation.

The timeline metadata codec is not a durable application recovery image. No
persistent timeline adapter, distributed atomic promotion or rollback of issued
effects is supplied. [Timeline verification](timeline-verification.md) and
[performance](timeline-performance.md) retain their measured scope.

## Recoverable execution

Task recovery permissions are `RestartFromBeginning`, `RestartFromCheckpoint`,
`ReconcileBeforeRestart` and `NeverRestart`. `ReconcileRequired` effects require
independent reconciliation after interruption in every mode. Unknown evidence
keeps recovery blocked; worker exit and transport acknowledgement do not prove
external success. Replay reconstructs decisions without executing task code.

Use the [operations runbook](reliable-execution-operations.md) for admission,
restart, fencing, leases, backups and reconciliation.

## Execution contracts

The [task contracts](../crates/penelope-core/src/task/contracts.rs) define
fingerprints and recovery/resource fields. The original v1 fingerprint encoding
and wire formats remain covered by frozen compatibility tests.

## Portable core and application integration

The portable core requires neither `std` nor allocation for supported caller-owned
state/buffer profiles. The hosted facade, supervisor and saga adapters remain hosted.
Allocation checks and the bare-metal cross-builds below qualify their stated
profiles, not arbitrary application types or whole-process memory use.

Applications own model determinism, snapshot completeness, migration authorization,
authentication, journal durability, persistent fencing, runtime enforcement and
independent effect evidence. Recovery cannot establish arbitrary program correctness.
See the [SQLite deployment profile](../crates/penelope-sqlite/README.md),
[Linux worker restrictions](../crates/penelope-process-runtime/README.md) and
[hosted integration contract](INTEGRATION.md). Process-kill tests do not certify
power loss; optional StateChronicle correlation does not authenticate its own input.

## Integration boundaries

StateChronicle schemas require explicit versioning and authorized migration;
custom transitions do not imply multi-resource atomicity. Its portable components
do not turn hosted execution into an embedded runtime. Optional signed-event and
checkpoint conformance has a pinned upstream and trusted fixture authority; see
[the conformance contract](INTEGRATION.md#optional-signed-canonical-event-conformance).
Deployment adapters need their own atomic commit/CAS, retention, idempotent-start,
fencing and external-evidence checks.

## Validation

Run from the repository root using pinned Rust 1.100.0 and supported MSRV 1.89.
CI selects stable, MSRV or nightly explicitly through `RUSTUP_TOOLCHAIN`.
The full gates also need Python 3.11+, nightly Rust with `cargo-fuzz`,
`cargo-llvm-cov`/`llvm-tools-preview`, `cargo-audit`, `cargo-deny` and `gitleaks`.
Install `riscv32imafc-unknown-none-elf` for both tested compilers; cross-compilation
is a portability check, not evidence from physical hardware.

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo test --workspace --doc --all-features --locked --exclude penelope-fuzz
cargo +1.89.0 test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
python3 scripts/check-nested-example.py
RUSTUP_TOOLCHAIN=1.89.0 python3 scripts/check-nested-example.py
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

Repeat the four portable-core/timeline checks with `cargo +1.89.0`. Run the
[external consumers](../ci/machine-consumer/Cargo.toml), compatibility harness,
frozen saga/task/timeline comparison and real old-writer SQLite upgrade on both
compilers; their exact commands and archived outcomes are in the
[current checkpoint](0.5/evidence/packaging/qpsg-candidate-checkpoint.json).
Optional signed-source checks use `scripts/run-statechronicle-conformance.py`
with the pinned upstream specified in that checkpoint.

The chaos command is the full three-round campaign, not the shorter CI smoke.
Keep per-round seeds and source hashes, and retain failures and interrupted
attempts. Coverage must meet the unchanged ratchet; passing aggregate coverage
is not proof of every failure branch. Qualify performance against original 0.4
using [plan §13.2](0.5/plan.md#132-baseline-performance-methodology), with explicit
disposition of every retained finding. Do not substitute debug benchmark smokes
or earlier-revision results for final source qualification.

## Source compatibility review

The [0.5 compatibility matrix](0.5/compatibility-matrix.md) covers old imports,
wire/replay fixtures, actual database upgrades and backup-based rollback.
The [static API review](0.5/evidence/maintenance/public-api-audit.json) supplements
signature checks with resolved export bindings and unchanged external consumers;
it records a reexport removal missed by the signature checker.

The historical 0.3 transition intentionally extended `ProcessInputKind` and
`ProcessAuthorizationOperation`. Exhaustive matches and authorization policies
required updates despite unchanged old numeric tags and wire names. The
[original source review](https://github.com/STEXS-Technologies/penelope/blob/c5bb7cf420e19eb7420391ae3e96e823444f4676/docs/RELEASE.md#source-compatibility-review)
retains that failing strict patch-level report and the compiler evidence for the
hosted `SupervisorError` reexport. Historical
[0.3](qualification-0.3.0.md) and [0.4](qualification-0.4.0.md) audits retain their
own source scopes and do not qualify this candidate.

## Packaging and publication

`./scripts/publish-crates.sh` verifies all ten library archives in a fresh temporary
Cargo registry and retains them under the configured target's `package` directory.
It excludes the fuzz package. Final qualification also tests an independent
workspace extracted from those archives, with every Penelope dependency resolved
from packaged content. Record source identity, locks and archive hashes.

After qualification and separate authorization, verify the intended release
revision on `main`, add the dated version entry to the changelog, and reconcile
all source/artifact identities before creating the annotated matching tag.
The release workflow verifies tag/version and changelog, publishes in inferred
dependency order and creates GitHub release notes from that changelog section.
Missing, empty or duplicate entries fail verification. Manual publishing requires
a release tag and explicit publishing input. Credentials stay outside the repo.

`./scripts/publish-crates.sh --publish` waits for exact registry versions, skips
already-published versions and verifies all expected versions. Its default mode
verifies packages without publishing. Do not tag or publish while a major gate
remains open.

The [previous full guide](https://github.com/STEXS-Technologies/penelope/blob/c5bb7cf420e19eb7420391ae3e96e823444f4676/docs/RELEASE.md)
preserves detailed historical validation, fingerprint encoding and performance
citations. Current decisions belong in the [verification record](0.5/verification.md),
not in duplicated release histories. Cache and hosted-memory methods remain in
[SQLite cache measurement](performance-sqlite-cache-method.md) and
[hosted-task measurement](hosted-task-memory-method.md).
