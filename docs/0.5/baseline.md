# 0.4.0 baseline for the unified execution refactor

Captured 2026-11-15 before production changes. F0 is in progress; this is not a
claim that every qualification profile has been rerun.

## Identity and environment

- Branch starting point: `42ec187c9adfc7403b06215f405f2575f53c35c2`.
- `v0.4.0` annotated tag object: `e774b61f3e51daa3b1df2eed6364450cedeb02aa`.
- Peeled tag commit: `42ec187c9adfc7403b06215f405f2575f53c35c2`.
- Rust/Cargo: 1.99.0; LLVM 23.1.1; host `x86_64-unknown-linux-gnu`.
- MSRV installed and checked: Rust 1.85.0.
- Linux kernel: `7.2.15-100.fc43.x86_64`.
- Portable target: `riscv32imafc-unknown-none-elf`.
- Existing untracked agent configuration and graph outputs are outside release changes.

[Package inventory](baseline/packages.json) records feature sets and direct
dependency kinds from `cargo metadata --format-version=1 --no-deps --locked`,
without local absolute paths. [Facade snapshot](baseline/facade.rs.txt) records
the original consumer exports. [SHA-256 inventory](baseline/source-sha256.json)
pins tracked crate sources, tests, scripts, workflow files, manifests and lockfile.
Original file bytes are recoverable with `git show <baseline-sha>:<path>`.
These inventories are not substitutes for behavioral compatibility tests.

## Checks executed on the unchanged production baseline

| Command | Result |
| --- | --- |
| `cargo test --workspace --all-features --locked` | Passed, including doc tests |
| `cargo +1.85.0 test --workspace --all-features --locked --exclude penelope-fuzz` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked --exclude penelope-fuzz -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |
| `./scripts/check-layer-boundaries.sh` | Passed |
| `cargo check -p penelope-core --no-default-features --target riscv32imafc-unknown-none-elf --locked` | Passed |
| `cargo check -p penelope-timeline --no-default-features --target riscv32imafc-unknown-none-elf --locked` | Passed |

The full CI command additionally selects `--all-targets`; that run and the
separate fuzz, chaos, coverage, conformance, supply-chain and representative
release-mode benchmark profiles remain pending. No timing/performance baseline
is established by the test runs above. See `.github/workflows/ci.yml` and the
proposal F0 for the complete required profile.

Cargo warns that the executor and protected-runtime examples both use the target
name `saga_recoverable_task`. Record this as pre-existing; qualify them separately
until deliberate source/tooling migration resolves the collision.

## Legacy consumer harness

`ci/compat-0.4` is an independent Cargo workspace with a frozen selection of
legacy facade imports, its own lockfile and a timeline feature. Run from root:

```sh
cargo check --manifest-path ci/compat-0.4/Cargo.toml --locked
cargo check --manifest-path ci/compat-0.4/Cargo.toml --all-features --locked
cargo +1.85.0 check --manifest-path ci/compat-0.4/Cargo.toml --all-features --locked
```

This checks downstream visibility and names. Extend coverage with separate
fixtures for signatures, exhaustive enum matching, protocol bytes and replay
before declaring F0 complete; do not edit frozen 0.4 usage to accommodate a 0.5
source break. A new canonical API gets a separate fixture.

## Semantic ownership map

| Layer | Owns | Keep separate |
| --- | --- | --- |
| core | Allocation-free task decisions and timeline planning identities | Task generations and timeline revisions |
| domain / intent | Saga DTOs, canonical bytes and inbound validation | Existing process v1 encodings |
| executor | Linear/graph saga decisions and hosted task composition | Compensation versus task recovery permissions |
| ports | Process commits, inbox/outbox, timers, hosted integration contracts | Process-specific commit authority |
| SQLite | Local durable process/task storage and worker transactions | Local durability versus external effect success |
| timeline | Scratch replay, immutable bounded history, publication and effect finality | Correction eligibility versus effect execution |
| process-runtime | Protected workers, IPC and generation fencing | Runtime observations versus verified completion |
| statechronicle | Canonical event verification/correlation boundary | External canonical authority |
| facade | Consumer imports over these layers | Compatibility aliases versus actual shared semantics |

## Follow-up evidence

The [initial verification checkpoint](verification.md) records completed stable
and Rust 1.85 all-target checks, signed conformance, coverage, representative
benches, the completed seed-one fuzz round and the deliberately interrupted
three-round campaign. It also records the baseline advisory failures and
subsequent approved Rust 1.89 dependency fixes. Commands above describe the old
baseline; run them at the baseline commit to reproduce the old MSRV profile.

The full unchanged-source default chaos campaign has now completed with exit zero:
three rounds, 1,000 property cases, and 22 fuzz targets per round with at least
10,000 executions each. [Raw evidence and counts](evidence/baseline-complete-chaos/)
supersede the earlier interrupted attempt. Remaining compatibility-fixture and
performance qualification gaps stay explicit in the verification ledger.
