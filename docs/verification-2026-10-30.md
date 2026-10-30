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
reproduction are retained. A 10,000-repetition replay using an explicit temporary
directory on the local filesystem is pending. The chaos campaign has not passed.
Existing [saga performance concerns](performance-parallel-followup-2026-10-30.md)
and [worst-case memory/stack qualification](performance-portable-memory-2026-10-30.md)
remain open. No coverage, correctness or package check closes those requirements.
