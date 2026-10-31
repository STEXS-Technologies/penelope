# SQLite replay page reservation — 2026-10-31

Replay reads now reserve the exact expected result count after validating the first row. The count comes from the process frontier in the same SQLite read transaction, bounded by the validated request limit. Empty pages allocate no result vector; single-row tail pages reserve one slot. Previously `Vec::push` grew the result vector geometrically.

All row type, size, checksum, scope, sequence, frontier and final count checks remain. No records or page sizes are cached across requests. Reservation happens after the first row passes its checks. The existing tests cover corrupted frontiers, checksum faults, request rebinding, appended outcomes, reopening, and scope isolation.

## Allocation evidence

Baseline is commit `794914678621a18f44f770d261b0bd052974f51f`. Both versions ran the unchanged release `process_replay_allocations` benchmark, with a 128-outcome history populated through public commits, 100 warmups per profile, and 16 measured calls per profile. Each returned page is checked against an independent history model outside the counting interval. One fresh benchmark process per revision; all 16 samples of each profile had identical counters.

| Profile | Baseline allocations / reallocations / requested bytes | Candidate | Reduction |
| --- | --- | --- | --- |
| First or last single outcome | 9 / 4 / 2,056 | 9 / 4 / 1,264 | 792 requested bytes |
| Full 128-outcome page | 9 / 9 / 67,528 | 9 / 4 / 34,792 | 5 reallocations, 32,736 requested bytes |
| End or past-end empty page | 8 / 4 / 1,000 | 8 / 4 / 1,000 | Unchanged |

Counters measure Rust `System` allocation traffic across the process while a public read is in flight. Requested bytes include reallocations, not live or peak memory, and exclude SQLite C allocations. The historical `copied_bytes` benchmark field is unchanged fixture metadata, not a measurement of this change. No latency comparison was performed: these results establish reduced allocation traffic, not a new overall Penelope slowdown percentage.

Raw samples, source hashes, exact benchmark command and verifier are in [the evidence directory](verification-data/2026-10-31-replay-reservation/). Run `python3 docs/verification-data/2026-10-31-replay-reservation/verify.py` to check all 160 samples.

## Validation

Qualification logs in the evidence directory record stable SQLite all-target/all-feature tests, the rest of the workspace all-target/all-feature tests (excluding fuzz), SQLite all-target/all-feature tests on Rust 1.85.0, and strict workspace Clippy. Formatting and layer boundaries were checked, and the local knowledge graph updated. The SQLite suites contain 33 unit, 60 process and 42 task tests. Debug benchmark timings in test logs are qualification output, not performance evidence.
