# Reliable execution verification — 2026-10-30

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

The longer three-property and three-iteration chaos/fuzz campaigns were still
running when this record was created; this record does not claim they passed.
Existing [saga performance concerns](performance-parallel-followup-2026-10-30.md)
and [worst-case memory/stack qualification](performance-portable-memory-2026-10-30.md)
remain open. No coverage, correctness or package check closes those requirements.
