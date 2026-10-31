# Task replay statement reuse evidence

Candidate code: `91cdfcf` (full pin in manifest). Baseline: `47c4a259e8a8c2bc23d848e36ff4ffb797c219a0`.
See [the report](../../performance-task-replay-statements-2026-10-31.md) for results, allocation tradeoffs, variability and qualification scope.

Root data describes the final design, which holds one blob statement throughout replay. `per-reference/` preserves the rejected trial that checked out a cached statement per reference. Each phase has its own primary/confirmation raw data, analysis, measured source snapshots, build/executable hashes and test logs. Do not pool phases or attribute prototype qualification to final production. The prototype's measured tasks.rs precedes rustfmt-only cleanup; `qualified-tasks.rs.gz` is its qualified formatted source. The final measured tasks.rs matches committed production source exactly.

Verify using Python's standard library:

```sh
python3 docs/verification-data/2026-10-31-task-replay-statements/verify.py
```

The verifier hashes all archived evidence, recomputes all 80 bootstrap statistics from compressed raw observations in temporary directories, checks process order/profile shape and allocation metrics, verifies 60 final regression logs, negative-control failure and restored-production tests, and checks qualification summaries and source/binary provenance. It does not reproduce hardware timing or independently authenticate recorded logs. Gzip streams use mtime zero. Binary payloads are represented by hashes rather than committed executables.

To reproduce final measurements, use a disposable checkout of the candidate commit in the recorded compiler environment. From the repository root create `target/task-replay-statements` and `target/qualification-temp`; copy root Python scripts and `confirm-provenance.json` here into `target/task-replay-statements`. Then run:

```sh
python3 target/task-replay-statements/build.py
python3 target/task-replay-statements/compare.py
python3 target/task-replay-statements/analyze.py
python3 target/task-replay-statements/prepare-confirm.py
python3 target/task-replay-statements/confirm.py
python3 target/task-replay-statements/analyze-confirm.py
```

Build scripts temporarily replace tasks.rs/lib.rs/Cargo.toml from pinned commits and restore them in `finally`. Run them only in a disposable checkout, without concurrent builds. Cargo JSON artifact messages identify exact executables. Current benchmark fixtures are used for both primary variants. `prepare-confirm.py` rebuilds historical control `50856b001f81978daea5044947557a26429d9740` and prepares the three confirmation executables. The historical control lacks required validation and must never be deployed as the final implementation. Its final rebuild was byte-identical to the reused measured executable (`rebuild-control.json`). Different environments/paths may change hashes; fresh runs require fresh provenance rather than treating archived timing as measurements of new binaries.

The original confirmation reused the verified prior historical binary, and baseline commit-benchmark identity matches the preceding scope-decoder candidate. Previous manifests/provenance preserve that chain. The unchanged commit fixture is archived and hashed. Primary runs clear/set all profile flags; confirmation controls the external-write flag. Run measurements with no project tests/builds, graph updates, profiling or bootstrap analysis; CPU 0 affinity and Linux `taskset` are required. Scripts overwrite target logs. All observations, including high-variance ones, are retained. Allocation figures count Rust allocations only, excluding SQLite's C allocator.

Final qualification used empty `RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER`, with `TMPDIR` set to the absolute `target/qualification-temp` directory:

```sh
RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz
CARGO_TARGET_DIR=target/worker-audit-msrv RUST_TEST_THREADS=1 cargo +1.85.0 test -p penelope-sqlite --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --exclude penelope-fuzz -- -D warnings
cargo fmt --all -- --check
bash scripts/check-layer-boundaries.sh
graphify update .
python3 target/task-replay-statements/repeat.py
```

`repeat.py` runs 60 isolated processes, 30 each on stable and Rust 1.85. Root `workspace.log.gz`, `msrv.log.gz`, `clippy.log.gz` and `repeat-*` describe final code. The negative-control log intentionally contains one failed test: ignoring the scope predicate serves another scope's identical-digest blob. `negative-control.py` restores source in `finally`, then runs the production scope regression again on both compilers; both final logs pass. Run this mutation separately with no concurrent builds. Full qualification preceded the intentional fault; exact measured source identity was verified again after restoration and historical-control rebuild.

To reproduce the rejected trial, start from a separate candidate checkout, replace tasks.rs/lib.rs/Cargo.toml with its candidate snapshots, use its archived build/compare/analyze scripts and current benchmark fixtures. No histories or memo limits change between primary variants. The final standalone blob-read path retains an uncached single query; reconstruction alone retains the blob cached statement across references.

Current local qualification excludes the separate fuzz crate and optional StateChronicle gate. The previous pushed commit's CI run completed successfully, including those separate checks; that result is not presented as final-code CI. No release plan from Downloads is included.
