# Reservation versus fence contention — 2026-10-31

Added a SQLite task authority regression test comparing two independent connections contending for the same generation. Runtime code is unchanged from `af30345`; the audit confirmed that reservation and fencing already use immediate transactions around their authoritative reads and writes.

Three initial states are tested: no reservation, an existing reservation for the attempted execution, and an existing reservation for a different execution. Each case constructs both serial reference histories through public APIs. Reference receipts are independently checked against the protocol: an unfenced fresh reservation returns `Reserved`, an identical existing reservation returns `AlreadyReserved`, and a tombstoned or differently reserved generation returns `Conflict`. Fencing must return the exact execution identity it preserves, or `None` if it won before any reservation. Task record/head/blob/retirement tables must remain exactly unchanged in the reference operations.

Each case then runs eight fresh database races. Both connections open before a three-party barrier releases the calls together. The test does not force lock-acquisition order. The pair of typed receipts and exact snapshots of all five task/authority tables must match a valid serial reference. The durable generation table must contain exactly one row with the expected generation, identity and fenced state.

A newly opened journal must reject a delayed start and an incorrect stop identity without changing any table. Correct stop confirmation must transition the row to state 2. Repeated fencing must preserve the identity and stopped state; repeated correct confirmation is idempotent; delayed start remains rejected. Original intent history is still readable.

## Evidence

Twenty isolated test executions passed, ten stable and ten Rust 1.85.0. They contain 120 serial reference histories, 480 barrier-released contention histories and 960 competing API requests, plus post-race identity/stop/reopen checks. The full task integration suite passed all 44 tests on both toolchains. Strict workspace Clippy covers all targets/features; formatting, layer boundaries and the local graph update passed. Other suites and benchmark binaries were not rerun for this test-only change.

Observed model matches across the twenty retained runs:

| Initial state | Reservation-first model | Fence-first model | Both models identical |
| --- | ---: | ---: | ---: |
| No reservation | 57 | 103 | 0 |
| Same execution reserved | 57 | 103 | 0 |
| Different execution reserved | 0 | 0 | 160 |

For the different-execution case, receipts and durable rows are identical in both serial orders; the test records `both` and does not infer an execution order. The other two cases observed both distinct models. This is bounded contention coverage, not proof of every possible schedule or successor-generation race.

## Fault control and reproduction

Temporarily changed the reserved/tombstoned fallback in `reserve_start` to acknowledge `Reserved` rather than reject it. The test caught the false acknowledgement in its independently checked fence-first serial reference (`Ok(Reserved)` instead of `Conflict`), before reaching the contention loop. Production source was restored byte-for-byte, then the final test passed again. This control proves the serial expectations detect a false start acknowledgement; it does not independently prove race-only defects are all detected.

[Retained evidence](verification-data/2026-10-31-authority-race/) includes the complete test snapshot, source hashes, all repeated histories and model counts, stable/MSRV suite logs, Clippy output, the control and restored-source run. Run `python3 docs/verification-data/2026-10-31-authority-race/verify.py` to check the retained results. No performance change is claimed.
