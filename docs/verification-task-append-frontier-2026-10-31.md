# Task append frontier verification — 2026-10-31

Baseline: `14f2ea2` (`Measure and document task record allocation reduction`). The exact baseline and final code commits are pinned in the evidence metadata.

A cached task projection was accepted for append when the stored head matched the cache's sequence, digest, and byte count. If retained rows had disappeared while that head remained intact, the cache could therefore authorize a completion over an incomplete history. This was reproduced by externally deleting the final record, or all records, from a committed two-record fixture. Across warm and explicitly rebuilt caches, four baseline cases acknowledged a new completion and changed durable rows. Two unexplained-tail cases returned `Conflict` rather than reporting corruption. All cold-cache cases already rejected the damage through reconstruction. These are injected retained-row losses, not evidence that a normal SQLite transaction spontaneously loses rows.

Append now uses the existing read-path query that compares head sequence with the maximum retained sequence in the same transaction snapshot. It runs inside the existing `BEGIN IMMEDIATE`, before trusting the cached projection and before attempting any writes. The query returns the head digest as a fixed-size owned `Digest`, preserving the SQL guard for the subsequent head update without an intermediate digest vector. Reads share that helper; checksum validation remains bounded and exact-length. The statement is cached, while its results are obtained afresh. This adds no full history replay to a healthy warm append. Schema, durability mode, rebuild and runtime authorization rules are unchanged.

The regression matrix injects six independent changes—missing head, lowered head, advanced head, missing final record, missing all records, and extra retained tail—against each of three cache states: warm after commit, warm after explicit rebuild, and cold after reopen. All 18 combinations must return `TaskPortError::Corrupt` and leave the five task/head/blob/retirement/runtime tables unchanged. Restoring authoritative test rows must then permit completion on the same journal, followed by reopened record and output-blob reads. Twenty isolated final runs, ten on stable 1.99.0 and ten on MSRV 1.85.0, exercise 360 rejection cases and their repair/reopen controls.

A separate release allocation probe reuses the preceding task-read fixture and validates its saved baseline executable hash against the earlier evidence. Each of the ordinary record, whitespace-padded 4 KiB record, and end-of-log profiles has 16 samples per version. All retain one Rust allocation, zero reallocations, and 192 requested bytes per call; SQLite C allocation and peak memory are excluded. This protects the preceding read-allocation improvement. It does not measure append allocations or latency, and no timing speedup or application overhead percentage is claimed for this fix.

The frontier check does not detect interior gaps below an intact maximum, changed interior records/blobs behind a matching warm cache, or coherently altered head/tail metadata. Full reconstruction remains necessary for broader verification. The explicit maintenance rebuild still handles a missing derived head according to its existing validation rules.

[Raw evidence and hashes](verification-data/2026-10-31-task-append-frontier/metadata.json) include the failing baseline, final tests, repeated fixtures, qualification logs, and read-allocation samples. Reproduce the regression with:

```sh
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo test -p penelope-sqlite --test tasks task_append_frontier -- --nocapture
RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= cargo +1.85.0 test -p penelope-sqlite --test tasks task_append_frontier -- --nocapture
```

Local qualification passed: workspace all-target/all-feature tests excluding `penelope-fuzz`, SQLite MSRV all-target/all-feature tests (24 unit, 51 process, 30 task), strict workspace Clippy, formatting, layer boundaries, and graph update. Separately, the preceding commit’s optional StateChronicle CI job failed a pure saga fixture with `Running` rather than `Completed` after its short polling loop; its failed job was requested to rerun at the same commit. This archive does not claim CI success or establish that failure’s cause.
