# Linux worker startup header allocations — 2026-10-31

The fixed 60-byte startup header now uses a stack array instead of a `Vec`.
This removes one measured Rust allocation from a real supervisor dispatch on
both supported toolchains. No latency or overall application-overhead reduction
is established by this allocation experiment.

Baseline: `fbcb4c725de6ba45f5c275f0444e8f213307bde4`.
Candidate: `40c674a`.

## Implementation and compatibility

`runtime::start_header` fills the existing wire layout: eight-byte `PNLWRK01`
magic, 32-byte scope digest, four-byte generation, and four big-endian u32
fields for metadata length, input length, checkpoint length, and checkpoint
presence. The caller retains checked length conversions and the existing
startup write deadline. Serialization, reservations, manifest durability,
sandboxing, input/checkpoint contents, and public APIs are unchanged.

The new unit test independently reconstructs the previous growable encoder and
legacy scope-digest input. It compares 10,000 deterministic bindings and length
combinations, including generation zero/MAX, zero lengths, 8192-byte metadata,
maximum u32 lengths, absent checkpoints, and present zero-length checkpoints.
Encoder edge cases are not assertions that callers admit oversized requests.
The test passed on stable and Rust 1.85.0.

## Real dispatch allocation evidence

The existing isolated `worker_protocol_allocations` executable now counts Rust
allocations around `TaskSupervisor::advance`, which commits Started, reads the
input, reserves the generation, writes and syncs the registry, starts the real
protected worker, and sends its startup frame. Fixture construction and submit
occur before this count. Successful checkpoint, durable checkpoint ACK,
completion, output verification, and fencing follow each dispatch.

| Compiler | Baseline dispatch allocations | Candidate dispatch allocations | Isolated processes |
| --- | ---: | ---: | ---: |
| Rust 1.99.0 | 245 | 244 | 10 baseline + 10 candidate |
| Rust 1.85.0 | 246 | 245 | 10 baseline + 10 candidate |

Every process produced the corresponding count. Only `runtime.rs` was swapped
between pinned baseline and candidate sources; the probe and other workspace
sources were identical. Both sources were restored through `finally` handling.
Compiler metadata, source and executable hashes, all 40 outputs, and the build
and comparison script are retained. Allocation counting covers parent-process
Rust allocator calls, not SQLite's C allocator or kernel/process memory. Counts
are fixture/compiler specific and are not a promise about every dispatch.
The existing zero-allocation checkpoint ACK/poll and 16-allocation repeated
public-fence controls continued to pass.

## Validation and retained evidence

Passed with compiler wrappers cleared:

- Full workspace all-target/all-feature suites excluding `penelope-fuzz`, with
  `RUST_TEST_THREADS=1`, on stable and Rust 1.85.0.
- Workspace Clippy with all targets/features and `-D warnings`.
- Formatting and layer-boundary checks.
- `graphify update .`.

Each compiler passed 29 runtime unit and 66 runtime integration tests, plus the
existing SQLite and remaining workspace suites. The prior baseline's CI run
`41160687878` passed. Candidate CI is tracked separately after push.

Evidence is in
[`verification-data/2026-10-31-runtime-start-header`](verification-data/2026-10-31-runtime-start-header).
The initial oracle run incorrectly used a different magic literal and failed;
its output is retained as `wire.log.gz`. The literal was corrected to the
existing protocol value before the passing oracle, comparison, and full checks.
This was a test-fixture error, not a baseline production failure.

```sh
python3 docs/verification-data/2026-10-31-runtime-start-header/verify.py
```

The verifier checks file hashes, source pins, complete compiler/variant/repeat
sets, allocation counts, control outputs, and stable/MSRV suite results. The
comparison script uses the original local paths and `HEAD` as its baseline;
reproduction requires the pinned baseline checkout with the candidate patch
applied before running it. Binaries are identified by hash and not stored.
