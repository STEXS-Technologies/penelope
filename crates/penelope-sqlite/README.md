# penelope-sqlite

Optional SQLite persistence for Penelope tasks and sagas. The default Penelope
facade does not depend on this package. The [0.3.0 qualification report](../../docs/qualification-0.3.0.md)
records the verified profile, evidence and limits.

`SqliteTaskJournal` implements synchronous `TaskJournal` and
`TaskGenerationAuthority` for one pinned task definition. `SqliteProcessStore`
implements the async `ProcessStore`, `Inbox`, `OutboxStore`, `TimerScheduler` and
`TimerClaimStore` ports through a bounded dedicated database worker.
`ProcessPayloadStore` retains scoped immutable event/evidence bytes for replay. Construct
these adapters explicitly; the library does not create application services.

## Storage profile

Use a single host and local filesystem with SQLite-compatible locking and sync
semantics. Opening requires WAL mode and sets `synchronous=FULL`. No shared
network filesystem or distributed consensus profile is implemented. Digests
provide corruption detection, not authentication against a database writer.
Configure filesystem ownership and privileged service authorization outside
this adapter. Verify canonical event evidence before binding its source identity
to an atomic process commit.

`SqliteLimits` bounds blob/record sizes, task history, process history, retained
scope usage, admitted scopes, database pages and busy waiting. The database page
limit covers the main database; leave disk capacity for WAL, temporary files and
backups. Long-lived external SQLite readers can prevent WAL reclamation. Task
and process scope quotas are accounted separately. Retired task identities
continue consuming scope quota so deleting bytes cannot allow resurrection.

Task hot appends reuse a validated projection while the observed head remains
unchanged. First use, switching scopes or observing another writer triggers
bounded replay. Read failures and missing committed records fail closed.

## Replay payloads

Write event bytes through `ProcessPayloadStore::put_payload` before committing
outcomes that reference their digest. Validate the returned digest before the
process commit. The write and outcome commit may use separate stores; a crash
between them leaves unreferenced bytes, never permission to infer a committed
event. Repeat the exact write after an ambiguous acknowledgement. Reconstruct
only from authoritative outcome references, then read and decode their payloads.
Missing, oversized or corrupt referenced bytes block replay.

Payloads bind the complete process definition scope and consume the shared
process storage quota, with per-payload and retained-count bounds. Schema version
4 adds this table transactionally without changing existing outcome bytes.
Payloads and unreferenced writes are retained; automatic payload garbage
collection is not implemented. Account for failed/orphan writes when sizing
retention. Content digests detect corruption and do not establish authenticity.

## Recovery and fencing

After an ambiguous task operation, reconstruct the supervisor. Do not infer
rollback from a failed acknowledgement. After an ambiguous process commit,
retry only the exact immutable batch to reconstruct its receipt or read and
replay authoritative outcomes; changed batches conflict. Receipt reconstruction
checks the exact retained outcomes, actions, input consumption digest/frontier and
canonical source binding, including the retained action effect-key index. Missing or inconsistent evidence blocks the retry; it
does not repair metadata or recreate delivery.

A generation authority reservation validates committed start authorization,
input, schema, checkpoint, generation and deadline. Repeating the same
reservation returns `AlreadyReserved`, which does not authorize a second worker
launch. `begin_fence` persists a tombstone and returns the reserved execution
identity. The trusted runtime must stop and verify that execution before calling
`confirm_stopped`. Successors are blocked while older reservations lack that
confirmation. This package does not itself terminate processes or undo issued
external effects.

Outbox and timer expiry permits redelivery. It does not establish that an effect
failed. Acknowledge deliveries only after independently establishing the
required downstream durable acceptance. Exact lease equality, persistent unique
tokens and inclusive expiry fence stale acknowledgement and renewal attempts.
Canceled timers remain retained and cannot be silently rescheduled. Claims compare
selected action IDs and effect keys, or timer IDs and due times, against the
validated payload before reserving any lease. Inconsistent selected metadata
rejects the entire batch without consuming a token. Claim updates must affect
exactly one row before a lease is returned. ACK and outbox renewal recheck the
retained payload/checksum, identity index, expiry and delivery metadata against
the lease. ACK must update exactly one row; an ignored write cannot report
durable success. Repeating a valid durable ACK remains idempotent.

## Async lifecycle

Construct `SqliteProcessStore` outside a latency-sensitive reactor; construction
opens and migrates synchronously. Its port methods enqueue bounded jobs and
await notifications. A full queue returns `QuotaExceeded`. Dropping a future
does not cancel an accepted database operation; its commit may still happen.
Dropping the store closes admission, and its worker drains already queued jobs
before closing its connection. Results are published before notification and
wake callbacks run outside the completion lock. An unwinding wake callback
cannot stop the worker from draining other accepted requests. No application
async runtime is required.

Supply a trusted `SqliteLeaseClock` in saga logical milliseconds, with a time
basis that remains consistent across restart. Task ticks are separate and must
come from the trusted task runtime/clock.

## Maintenance

Schema migrations run transactionally under a write lock. Newer schema versions
are rejected. Back up before an upgrade, close competing service writers and
retain the previous database until the upgrade has been validated. Never deploy
an older adapter against a newer schema.

Schema version 5 adds partial indexes for pending outbox actions and pending
timers ordered by due time. Completed history and future timers no longer need
to be scanned to select a due batch. Indexes consume database pages and add
maintenance work to inserts and acknowledgements; the database page limit still
applies during upgrade. A failed upgrade rolls back both indexes and the schema
version. Pending but unexpired leases can still require scanning within the
pending index. These indexes do not implement saga-history retention.

`check_integrity` checks SQLite structure. Replaying task histories also checks
record order and referenced blob identity. On corruption, stop dispatch,
preserve the database and WAL, and recover from authoritative backup/evidence.
`rebuild_head` repairs derived task head metadata only after validating the log
and blobs; it rejects a log missing its known committed tail. It cannot recover
missing authoritative records or authenticate rewritten records.

`retire_task(scope, expected_next_sequence)` explicitly removes a terminal task's
records and blobs only after validating the frontier and confirmed termination.
Satisfy audit, backup and application retention requirements first. Retirement
retains its identity and fence tombstones. Saga history is retained; automatic
saga-history garbage collection is not implemented.

`checkpoint_wal` requests WAL truncation during maintenance. A nonzero busy
result is not proof of reclaimed space. Deleted main-database pages are reused;
the method does not shrink the main database file. Take backups with a
SQLite-consistent procedure, or fully stop writers and close all database
connections before copying database files. Exercise restore in an isolated
location before reconnecting dispatchers.
