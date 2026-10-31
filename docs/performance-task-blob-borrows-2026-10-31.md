# Borrowed verified task blobs — 2026-10-31

Baseline: `756724d8edc814743f45f6af115fb4a9f3818565`. Exact code and source/binary pins accompany the evidence.

Task blob consumers now run inside a private callback while SQLite's bounded BLOB column remains borrowed. Digest and declared length verification finishes before the callback can copy data into a caller buffer, compare an existing blob during commit, or accept a referenced blob during reconstruction. This removes the temporary Rust vector while preserving SQL error classification, missing/corrupt blob rejection, configured limits, transaction boundaries and caller-owned output. Healthy public reads still copy into the caller's buffer; this is not a claim of no copying inside SQLite or across the whole operation.

A 24-pair alternating AB/BA campaign ran 192 processes: ordinary-allocator blob latency, separate blob allocation traffic, ordinary-allocator authorization-retry latency, and separate retry allocation traffic for each variant. All ran on CPU 0 of an AMD Ryzen 9 7950X, Linux 7.2.9-100.fc43.x86_64, release Rust 1.99.0. No agent-owned builds, tests or graph jobs ran during timing. The complete campaign retains 528 latency observations and 8,448 allocation samples across 11 profiles.

Blob fixtures publicly commit an input intent to a fresh WAL/FULL database, then call public `read_blob` into a preallocated buffer. Healthy and corrupt rows are separate fixtures; corrupt rows contain different same-size bytes, except the empty-reference case stores one invalid byte. Each profile warms 16 calls and times 1,000 calls, or 128 at 1 MiB. Return code/length checks are included in the timed loop; full buffer checks and retained BLOB checks occur outside timing. Corrupt reads must leave the buffer unchanged. Allocation probes use 16 measured calls per process.

Authorization retry fixtures reuse the preceding public `reserve_start` benchmark: 2, 16 or 128 retained records (intent, start, repeated checkpoints), five-byte input/checkpoint blobs, one existing reservation, 16 warmups and 250 timed calls. Every retry fully reconstructs authorized history. These are incremental changes relative to the preceding reconstruction optimization; percentages must not simply be added to that campaign's gains.

| Operation/profile | Paired mean latency change | Bootstrap 95% interval | Rust allocations before → after | Requested bytes before → after |
|---|---:|---:|---:|---:|
| Blob 0 bytes, healthy | -0.718% | -1.340% to -0.097% | 0 → 0 | 0 → 0 |
| Blob 0 bytes, corrupt | -1.313% | -2.036% to -0.669% | 1 → 0 | 1 → 0 |
| Blob 32 bytes, healthy | -1.108% | -1.875% to -0.444% | 1 → 0 | 32 → 0 |
| Blob 32 bytes, corrupt | -0.560% | -1.180% to +0.189% | 1 → 0 | 32 → 0 |
| Blob 4,096 bytes, healthy | -1.285% | -1.733% to -0.858% | 1 → 0 | 4,096 → 0 |
| Blob 4,096 bytes, corrupt | -1.182% | -1.670% to -0.720% | 1 → 0 | 4,096 → 0 |
| Blob 1,048,576 bytes, healthy | -3.372% | -3.529% to -3.218% | 1 → 0 | 1,048,576 → 0 |
| Blob 1,048,576 bytes, corrupt | -3.524% | -3.645% to -3.403% | 1 → 0 | 1,048,576 → 0 |
| Authorization retry, 2 records | +0.043% | -0.769% to +0.821% | 3 → 2 | 69 → 64 |
| Authorization retry, 16 records | -0.918% | -1.418% to -0.413% | 17 → 2 | 139 → 64 |
| Authorization retry, 128 records | -1.752% | -2.173% to -1.305% | 129 → 2 | 699 → 64 |

Negative latency changes indicate faster calls. Percentages average each pair's relative change; bounds resample 24 pairs 20,000 times with a fixed seed. All samples are retained. The 32-byte corrupt-read and two-record retry intervals include zero. Empty healthy reads remove no allocation, so their measured timing difference is not attributed to allocation savings. No application-wide slowdown percentage, universal latency guarantee or arithmetic combination with earlier gains is established.

Every allocation profile has 384 samples per variant, all agreeing exactly. Reallocations are zero. Direct reads use no temporary Rust allocation in the measured operation after the change; caller buffers, setup, SQLite C allocation and peak memory are excluded. Nonempty direct reads remove one allocation of the stored blob length; corrupt empty reads remove one allocation of one byte. Reconstruction removes one five-byte allocation per referenced record: 1, 15 or 127 allocations, respectively. Its two remaining Rust allocations request 64 bytes total. These results do not claim whole-program allocation freedom.

Both versions pass a ten-case matrix: valid, wrong-content, empty, short, long, over-limit, TEXT, INTEGER, REAL and missing stored blobs. It checks caller buffers and durable rows, reconstruction rejection, duplicate-row comparison rejection where a retained row exists, authoritative repair, successful duplicate-blob checkpoint append, too-small caller buffers and reopened reads. Missing rows exercise read/reconstruction rejection, not the duplicate-row branch. Separate empty and maximum 1 MiB cases verify caller-buffer suffixes, corruption rejection without buffer writes, repair, reconstruction and reopen. Twenty isolated final runs (ten per compiler) cover 200 matrix cases plus 40 boundary fixtures.

[Raw evidence](verification-data/2026-10-31-task-blob-borrows/campaign.json) includes all rows, exact summaries, successful preflights, code/source/binary hashes and scripts. To reproduce, copy this directory to `target/task-blob-comparison`, check out the pinned code commit, run `build.py` from the repository root, then `compare.py` and `analyze.py` there. The builder temporarily installs the archived baseline production source and restores the candidate in `finally`. Avoid other local jobs during timing. Broad qualification benchmark output is smoke validation only.

Final qualification passed: workspace all-target/all-feature tests excluding `penelope-fuzz`, SQLite MSRV all-target/all-feature tests (24 unit, 51 process, 32 task), strict workspace Clippy, formatting, layer boundaries and graph update. All seven optional StateChronicle signed-source/checkpoint/saga tests passed against pinned revision `ae3d94bc22c7835eaa3fdcd766e3c117929c12bb`, including SIGKILL recovery. The evidence verifier recomputes all 11 timing summaries and allocation deltas and checks source/data hashes and all 20 repeated fixture runs.
