# Fixed SQLite task scope keys — 2026-10-31

Source: `5d367932358c638cb81d881c3d4bc855a87fb96b`. Baseline: `7acf97f7a6a0bda6a5c0b60dc5eaa201713efad5`.

The private SQLite task scope key now uses a 128-byte stack array instead of a
heap vector. The same tenant/task/definition/input digest bytes bind as a SQLite
BLOB through rusqlite's fixed-array ToSql implementation. Query predicates,
validation, transactions, WAL/FULL durability and stored schema are unchanged.
This applies to task journal, generation authority and retirement key construction.

## Allocation result and compatibility

The existing real protected-worker allocation probe measures each of three
repeated public fences after completing/checkpointing/fencing its worker.
Ten separate processes per variant on both Rust 1.99.0 and 1.85.0 give 40
processes and 120 fence counts: **24 allocations per fence before, 16 after**.
Setup, protected execution and the initial fence are outside these counts. The
surrounding operation still allocates. These are global Rust allocator counts;
C-library allocations are not counted.

The legacy-key oracle matches all bytes for 10,000 varied scopes per compiler,
with each scope coordinate independently varied to check separation. Another
256 cases per compiler check SQLite typeof/length/content, insert with legacy
vector keys and read/update with arrays, then read the updated value using legacy
keys. Both binding forms select the same persistent row; no migration is needed.

## Timing result

All 16 measured timing intervals include zero. **No wall-clock speedup is
established** by this campaign. The change is retained for its verified reduction
in heap allocations and exact persistent-key compatibility; timing results do
not exclude smaller improvements or regressions. These comparisons do not
measure Penelope-versus-direct-execution overhead.

| Profile | Baseline ms | Fixed-key ms | Paired change | Bootstrap 95% interval |
|---|---:|---:|---:|---:|
| Live, same CPU | 1.3094 | 1.3271 | +0.02% | [-1.19%, +1.10%] |
| Exited | 1.2511 | 1.2526 | +0.33% | [-0.93%, +2.46%] |
| Repeated fence | 0.0889 | 0.0894 | -0.62% | [-3.43%, +2.27%] |
| Live, other CPU | 1.3158 | 1.2949 | +0.07% | [-1.67%, +2.36%] |
| Exited control | 1.2514 | 1.2406 | -2.48% | [-3.92%, +0.19%] |
| Repeated, other-CPU history | 0.0902 | 0.0885 | -0.75% | [-3.84%, +0.07%] |
| Full workflow, fresh runtime/store | 6.8007 | 6.3858 | -2.81% | [-13.40%, +2.91%] |
| Full workflow, reused runtime/store | 6.1197 | 5.9404 | +0.88% | [-11.38%, +3.96%] |

The campaign uses 20 alternating pairs, one full-process warmup per binary/case,
32 samples per fence profile and 16 per dispatch profile. It contains 126
processes and 9,450 worker fixtures including warmups. Both variants use the
same current release checkout, substituting only baseline tasks.rs for the
baseline build. The additional key unit-test module is excluded from benchmark
builds. Benchmark and allocation fixtures are unchanged.

Parent CPU is 0. Ordinary workers inherit CPU 0; the other-CPU case pins its own
live/repeated child to CPU 1 before timing. Exited controls remain on CPU 0;
a repeated fence times an already fenced generation with no running worker.
No own builds, tests or graph updates ran during timing. Host load, frequency
and cache state were uncontrolled. Metadata retains toolchain, topology and
binary hashes.

Public fence timing includes registry locking/manifest checks, WAL/FULL authority
begin/confirm and worker termination/reaping as applicable. Setup/startup and
post-fence safety assertions are excluded. Full workflow timing includes protected
execution, checkpoint acknowledgement, durable completion and fencing, using the
existing dispatch benchmark's boundaries.

Displayed medians are medians of process medians. Paired change is the median
of per-pair percentage changes and need not equal the difference between the
displayed medians. Negative means faster. Bootstrap uses 10,000 resamples,
seed 3030; intervals are descriptive and uncorrected for multiple comparisons.
All raw samples and all 16 summaries are retained, including setup/intent/start/
checkpoint metrics. Near-zero reused-store setup is a timer-scale control;
its percentage is not a useful speed measurement.

## Qualification and reproduction

Full workspace tests pass on Rust 1.99.0 and 1.85.0, including 18 SQLite unit
and 66 runtime integration tests. Strict workspace Clippy, formatting, layer
checks and AST graph update pass. Initial test-only FromSql type and binding-name
errors were corrected before final qualification; their logs are retained.

Tests use empty RUSTC_WRAPPER/RUSTC_WORKSPACE_WRAPPER,
`TMPDIR=/home/ac/projects/penelope/target/qualification-temp` and
`RUST_TEST_THREADS=1 cargo test --workspace --all-targets --all-features --exclude penelope-fuzz`.
MSRV uses `cargo +1.85.0` with `CARGO_TARGET_DIR=target/worker-audit-msrv`.
Clippy uses the same workspace/target/features/exclusion flags and `-- -D warnings`.
Other checks: `cargo fmt --all -- --check`, `bash scripts/check-layer-boundaries.sh`,
`graphify update .`.

[Retained evidence](performance-data/2026-10-31-sqlite-scope-key/evidence.json)
includes full outputs, allocation counts, qualification/build logs, scripts,
source and binary hashes, deterministic summaries and artifact hashes. Run:

```sh
python3 docs/performance-data/2026-10-31-sqlite-scope-key/verify.py
```

For a disposable source checkout, create target/qualification-temp and copy this
evidence directory's compare.py to target/compare-fence-wait.py before running
build.py (which restores tasks.rs in a finally block). Then run the emitted
comparison runner with `--pairs 20 --samples 32 --dispatch-samples 16` and a fresh
`--output` directory. compare-allocations.py reproduces the isolated allocation
counts. Adapt fixed CPU/TMPDIR paths to another host and record those changes.

The fixed storage belongs to the optional SQLite host adapter. The portable
controller and its previously measured stack bound are unchanged. No public
API or persistence-format change is introduced.
