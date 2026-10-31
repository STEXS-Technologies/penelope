# Timer deferred COMMIT failure audit, 2026-10-31

Timer scheduling, cancellation, due claims and acknowledgement now have explicit regression coverage for a failure at SQLite `COMMIT` after successful writes. The existing production implementation correctly reports these failures and rolls back. This change adds tests and evidence without changing runtime code or performance.

Sixteen cases cover all four operations, short/maximum-length process scopes, and one/four actions. Scheduling starts with the target absent and other timers present. Cancellation and acknowledgement start with all timers leased. Claims start with all timers due and unleased. An `AFTER INSERT` or `AFTER UPDATE` trigger inserts a deferred foreign-key violation and invokes a SQL callback on the worker connection. The callback observes each completed mutation, including all four writes of a larger claim, before the final commit fails with `PortError::Conflict`.

Every failed operation returns the error, restores autocommit, and preserves an exact logical snapshot of the ten process tables and two probe tables. This proves rollback includes schedule quota charges, token-counter advances, lease bytes, delivery attempts, expiry and terminal state. The test repairs the missing foreign-key parent while keeping the trigger, retries on the same worker, and confirms the expected writes execute again and commit successfully.

Independent checks verify the exact schedule quota increment (serialized timer length plus 128 bytes, charged once), unchanged usage for other operations, the next token and each lease token, every stored envelope, attempt, expiry and terminal state. For four-action cancellation/acknowledgement cases, the three other timers remain unchanged. Other process tables remain identical. A new adapter connection retains the successful snapshot, accepts a duplicate schedule without another charge, finds no eligible unexpired leases, and rejects acknowledgement of the cancelled lease. After removing the test trigger, current leases can finish through that new adapter; every timer is then confirmed terminal.

These cases cover fresh due claims, cancellation of leased timers and first acknowledgements. Existing tests separately cover reclaim, statement failures, cancellation races and duplicate semantics. Deferred constraints exercise commit-error handling and rollback; they do not simulate filesystem I/O faults, disk-full conditions or process death.

## Qualification and evidence

Twenty stable repetitions and twenty Rust 1.85 repetitions execute 640 explicit fault/retry/reopen cases. Six negative controls are rejected: ignored commit errors for each of the four operations, disabled drop rollback leaving a transaction open, and a schedule quota charge one byte too large. The driver restores production source in a `finally` block, with no other project build, test or graph jobs running during source mutations.

Final stable all-feature workspace tests excluding the fuzz package, Rust 1.85 all-feature SQLite tests, workspace all-target/all-feature strict Clippy, formatting and layer checks pass. SQLite has 44 unit tests, 72 process integration tests and 45 task integration tests. Graph output is refreshed locally and stays outside the commit.

[Archived logs, source hashes, drivers and read-only verifier](verification-data/2026-10-31-timer-commit-boundary/) retain all repeated results and failing negative controls. Run `python3 docs/verification-data/2026-10-31-timer-commit-boundary/verify.py` at this version. The verifier checks the evidence manifest, source identities, unchanged production sources and appended test relative to parent `459780941807379174f15b4c3a79a321d9cb758a`, all repetitions, negative-control failure markers and qualification results.
