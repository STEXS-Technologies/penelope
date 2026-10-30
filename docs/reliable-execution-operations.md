# Operating reliable execution

This runbook covers the optional single-host SQLite and static Linux x86-64
worker profile. Release qualification is ongoing. It does not establish a
production deployment, distributed fencing, or exactly-once remote effects.
See the [SQLite profile](../crates/penelope-sqlite/README.md) and
[worker restrictions](../crates/penelope-process-runtime/README.md) before setup.

## Bootstrap and admission

1. Create a service-owned private local database directory and runtime registry.
   Keep their ownership and paths stable across restart. Use a filesystem with
   SQLite locking/sync semantics; allocate disk headroom for WAL, temporary files,
   retained histories, payloads and backups, beyond the main-database page cap.
2. Configure `SqliteLimits` and `ProcessRuntimeConfig` explicitly. Bound history,
   payloads, admitted scopes, worker count, executable/frame sizes, queue depth,
   busy waiting and startup/fence timeouts. Verify the worker is a supported
   static ELF and pin its `code_identity()` in the task definition. Confirm the
   host supports the required kernel facilities; unavailable facilities reject
   admission rather than falling back to unprotected execution.
3. Open/migrate stores outside a latency-sensitive async reactor. Inject the
   durable generation authority into `LinuxProcessRuntime`, using the same task
   journal and private registry after reconstruction. Keep the spawning thread
   alive while workers run: Linux parent-death signals are thread-affine.
4. Supply authenticated principal identity and a `ProcessAuthorizer` for service
   calls. A scope, digest or lease token is not authentication. Pin the process,
   task, input, worker, classifier and independent effect-evidence contracts.
   Verify committed canonical events before binding their source identity.
5. Establish trusted clock mappings. Saga logical milliseconds, task ticks and
   kernel CPU seconds are distinct. Lease time must remain comparable across
   service restarts; retained task deadlines require a consistent tick basis.
   Do not reset either time basis merely because a new process started.
6. Reconstruct retained state before admitting deliveries. Use bounded
   `claim_outbox`, `dispatch_outbox`, `drive_task_once`, `reconcile_effect`,
   `drain_timers` and `relay_task_result` calls from the host's own scheduling
   loop. Apply backpressure and record failures; the library does not create an
   autonomous scheduler.

The executable reference is
[`saga_recoverable_task`](../crates/penelope-process-runtime/examples/saga_recoverable_task.rs).
Its fixed principals, task logic and evidence fixtures must be replaced by the
application's trusted policies and independent evidence sources.

## Crash restart and ambiguous acknowledgements

Stop new admission for the affected scope. Reopen the authoritative stores and
reconstruct the supervisor with the original scope, definition and registry.
A retained running generation is fenced before interruption is recorded.
Any storage/runtime ambiguity poisons the current supervisor; discard that
instance and reconstruct rather than continuing its in-memory projection.
A runtime drop or a worker's exit message does not confirm a durable fence.

For an ambiguous process commit, retry only its exact immutable batch to recover
the receipt, or replay retained outcomes and their payload references. Do not
change input IDs, action keys or source identities to get past a conflict.
For an ambiguous task commit, recover bytes and records together from the journal.
A failed response does not prove rollback. Missing or corrupt referenced evidence
blocks the scope; it is not an empty history or permission to redispatch.

A task completion is a proposal boundary, not independent proof that an external
mutation succeeded. Validate output with the pinned classifier and obtain the
required independent effect evidence before relaying an application success.

## Unknown effects and operator handling

Keep a `ReconcileRequired` task blocked while its external outcome is unknown.
An operator investigates the original stable effect key through the independent
executor/evidence source; do not launch a replacement effect under a new key.
Use the authorized reconciliation path and validate exact tenant, process,
action, task, input, generation and evidence binding. Unknown evidence remains
unknown through restart. A digest of worker output or a successful worker exit
cannot resolve the uncertainty.

If evidence authorizes restart, retain that evidence and its durable decision
before driving the next generation. If it proves success, relay only the
verified result. Escalate unavailable or contradictory evidence for application
resolution; administrative authority alone is not proof of an effect outcome.

## Lease reclamation and timers

After a claimant dies, allow its retained lease to expire against the trusted
lease clock. Claim the same scoped action/timer with a new unique token and
bounded attempt count. Reject stale ACK/renewal tokens, including at expiry.
Expiry establishes eligibility for redelivery, not failure of a remote effect.
Reconcile or use the destination's stable idempotency contract before repeating
an effect whose acceptance was ambiguous.

Timer delivery persists immutable payload and deduplicated inbox acceptance
before ACK. After restart, inspect accepted, unconsumed inbox items before
redelivery. The saga must still commit the decision that consumes the timer;
a delivery ACK alone does not prove that decision happened. Preserve canceled
timer identities instead of recreating them with altered delivery IDs.

## Upgrades, retention and backup/restore

Quiesce admission and service writers before upgrading. Take a consistent backup,
retain the prior deployment and database, then open the new adapter and validate
schema/integrity plus bounded replay before restarting service loops. Migrations
are transactional; never run an older adapter against a newer schema or edit the
schema version manually.

Retire task data only through `retire_task` after terminal-history validation,
confirmed fencing, expected-frontier checks and application audit/retention
approval. Keep identity/fence tombstones: deleting them permits stale work to
reappear. Saga histories and process payloads, including orphan payload writes,
are retained; automatic saga/payload garbage collection is not implemented.

For a file-copy backup, stop writers, establish confirmed worker termination,
close database connections, and preserve the matching registry with the database.
Alternatively use a SQLite-consistent backup procedure with an application-defined
coordination protocol; copying only an active main database file is insufficient.
WAL truncation is maintenance, not a backup, and a busy result does not prove
space was reclaimed.

Exercise restore into an isolated location with dispatch disabled. Validate
schema, integrity, histories, referenced bytes, definition/code pins, fence state,
registry identity and clock mappings. An older backup can omit later effect
acceptances and fencing decisions. Reconcile external effects and establish that
no original deployment can still execute before reconnecting any dispatcher.
Do not activate two copies of the restored authority against the same effects.

## Fence failures and corrupt evidence

On an unconfirmed stop, keep successors blocked and preserve the database, WAL,
registry manifests and bounded error context. Retry reconstruction/fencing using
the original registry and trusted kernel identity. Never clear reservations,
delete manifests or call `confirm_stopped` based solely on an operator assertion.
That confirmation belongs to the runtime after verified termination.

A damaged manifest, mismatched identity or missing authoritative history requires
investigation or recovery from consistent evidence. `rebuild_head` repairs only
derived task metadata after validating the authoritative log and blobs; it cannot
repair missing records, authenticate rewritten history, or prove termination.
If verified recovery is unavailable, leave the scope blocked and escalate.

## Telemetry and tenant isolation

Record bounded status/generation changes, authorization denials, typed error
categories, lease attempts/expiry, queue saturation, corruption, unresolved
effects and fence failures. `TaskDriveReport`, `TimerDrainReport` and timer error
stages expose progress without inferring success from a swallowed error. Alert
on prolonged unknowns, repeated fence failures, quota pressure and corruption.
Use a fixed metric label set for operation, status and error category; put scoped
identities in access-controlled bounded audit records instead of high-cardinality
metric labels. Do not log payloads, worker input, evidence bytes or credentials.

Validate full tenant/process/task scope at each service boundary and retain the
same binding during replay and deduplication. Authorize principals per operation;
shared-database scope checks do not replace filesystem ownership or host access
control. Separate database/registry directories where the deployment requires
stronger tenant separation. The trusted host kernel and database writer remain
inside this profile's trust boundary.
