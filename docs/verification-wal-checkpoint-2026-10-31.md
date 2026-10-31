# WAL maintenance with a retained reader — 2026-10-31

Added a disk-backed regression test for `SqliteTaskJournal::checkpoint_wal`. Production code is unchanged from `34edf6d`. Earlier maintenance coverage checkpointed only after readers had released their snapshots.

The journal publicly commits intent/start records. A separate SQLite connection begins a deferred transaction and reads both records, establishing its snapshot. The journal then publicly commits completion and its output blob. The test configures zero busy timeout, with no sleeps or timing-dependent coordination.

The first TRUNCATE checkpoint must report `busy == 1`, retain nonempty WAL bytes, and show a positive frame count with fewer checkpointed frames. Exact snapshots of the five task/fence/retirement tables stay unchanged. The retained reader must still see two records while the journal sees the newly committed completion.

After explicitly rolling back the reader transaction, another checkpoint must report success and zero frame counts. The physical WAL file length must be zero. The released reader now sees three records. The completion output is verified in a caller-owned buffer with its unused sentinel tail intact. A newly opened journal reads the intent and completion, rebuilds its head without changing any snapshotted table, and passes structural integrity checking.

## Evidence

Twenty isolated scenarios passed: ten stable and ten Rust 1.85.0. Both complete task integration suites passed all 43 tests. Strict workspace Clippy covers all targets/features; formatting, layer checks and the local graph update passed. Other suites and benchmark binaries were not rerun for this test-only change. This exercises orderly snapshot retention/release and physical WAL truncation, not process death, power loss or full-disk behavior.

A deliberate control replaced only the returned busy value with zero. SQLite still encountered the retained reader and kept a nonempty WAL; the test rejected the false-success acknowledgement (`0` instead of `1`). The production source was restored before successful qualification and repeated runs. This control establishes detection of a misleading busy status, not every possible maintenance fault.

[Retained evidence](verification-data/2026-10-31-wal-checkpoint/) includes every repeated scenario log, stable/MSRV suite logs, Clippy output, source hashes and the exact control. Run `python3 docs/verification-data/2026-10-31-wal-checkpoint/verify.py` to check the retained results. No performance change is claimed.
