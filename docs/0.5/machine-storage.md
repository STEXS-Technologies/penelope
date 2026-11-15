# Scoped machine journal and local SQLite profile

The hosted `penelope::storage::machine::MachineStore<INTENTS>` now has a concrete
`penelope_sqlite::SqliteMachineStore<Policy, INTENTS>` implementation. It stores
canonical accepted batches from the native owner, not a second machine evaluator.
The adapter also implements leased outbox delivery, source-authorized
acknowledgement and typed native task/saga admission, described below. These
implemented contracts remain subject to final release qualification; see the
[canonical recovery and composition tutorial](../getting-started.md).

## Admission and authority

`prepare_machine_bootstrap` checks the pinned model/codec and genesis invariants
before retaining bounded bytes. Its private-field request can be shared by Arc
for acknowledgement-loss retry. Bootstrap never resets an existing machine:
exact repeated genesis succeeds even after subsequent commits; a different
initial state conflicts and a different pinned definition is refused.

`MachineStore` exposes bootstrap, atomic commit, coherent head snapshots, exact
committed-event lookup and bounded history. `MachineAuthorizer` has a host-owned
associated context and domain-specific `MachineOperation` values. Every operation,
including reads and duplicate retries, authorizes before querying scoped storage.
There is no default allow-all policy, payload-derived host identity or change to
legacy process authorization enums. Applications authenticate their host context;
a typed tenant assertion by itself is not authentication.

An immediate local transaction checks the pinned scope and coherent head, resolves
an exact existing event before comparing predecessor CAS, and otherwise checks the
native revision and complete predecessor-state digest. It writes the event bytes,
new state snapshot, canonical record and every ordered intent, then updates the
head and resource counters. It reads back the coherent head, complete batch and
usage counter before committing and issuing a success receipt. Required single-row writes use the existing checked
helper, so a trigger silently suppressing a write aborts the transaction. Intent
identities remain unique throughout a machine's retained history. A duplicate
compares the entire header, content identity, event/state and ordered metadata/
payloads; different content under the same event ID conflicts.

The returned receipt binds full scope/new revision, event and complete batch
identity. `PreparedMachineCommit::publish` verifies those coordinates before
consuming the original local handle. A committed receipt attests to that past
batch, not perpetual ownership of the latest head. Failed or lost acknowledgement
requires authoritative lookup/retry of the retained exact batch, never guessed
success or re-evaluation at a newer head. SQLite cannot establish authority over
external settlement or make a hash authenticate a maliciously rewritten database.

## Storage, recovery and bounds

Forward-only schema version 7 adds `machine_heads`, `machine_events` and
`machine_intents` in the existing migration transaction. Existing task/process
rows and formats are unchanged. Tenant plus instance selects a 64-byte key; the
head pins the complete 104-byte scope, including definition and both schemas.
Native revisions use fixed eight-byte big-endian SQLite blobs for ordered indexes;
the independent public machine wire keeps its specified little-endian integers.
There is no signed-SQL-integer reinterpretation of native revision identities.

Head reads use one SQLite snapshot and verify the indexed event tail, last
record/state, native revision counter, digest and genesis/receipt consistency.
Lookup validates the coherent head and exact stored commit. Ordered intent reads
check identity, ordinal, payload charge and hash; extra or missing rows fail.
Blob bounds apply before copying SQL columns into owned memory. The checked
`MachineStoredCommit` decoder then verifies the complete wire/payload binding.
`MachineSnapshot::restore` and stored-commit restoration share one native
canonical snapshot restoration helper. Neither evaluates events nor dispatches
intents. Local provenance/pending caches start empty; journal bytes retain the
durable intent history. The optional leased outbox below recovers delivery state separately from
this snapshot helper.

Payload limits bound each record, state, event, intent and complete batch. Shared
SQLite limits additionally cap machine scopes, events per machine, logical retained
bytes and database pages. Logical journal charge is genesis plus current head-state
bytes plus all retained complete batches; replacing the head subtracts the previous
head-state charge. It does not measure SQLite row/index overhead: the separate
page limit bounds database storage. Scope count is for machine scopes; existing
process/task counters retain their own semantics. Event history is append-only
and refuses admission at its limit. There is no automatic eviction that discards
event deduplication evidence or pending intent identity.

History requests pin an explicit upper native revision, exclusive lower revision,
record count and byte budget. Results are contiguous and ordered. A partial page
returns the last admitted revision as the next exclusive lower bound; concurrent
later writes do not change the captured range. An oversized first record fails
instead of disappearing; holes, inconsistent scopes and damaged records fail
closed. Zero budgets, ranges above the authoritative head and invalid bounds are
refused. Reads do not repair or rewrite history.

Opening/migration remains synchronous and belongs outside latency-sensitive async
reactors. All port calls execute SQLite work on the existing bounded
`DatabaseWorker`, using the shared local-filesystem WAL/FULL setup, page limits
and busy timeout. Queue saturation/unavailability can leave a commit outcome
uncertain, so callers retain exact immutable batches for authoritative resolution.

Commit obtains an immediate transaction before checking duplicates or stale heads.
For a hot instance, sequence command admission through one host owner while
retaining the store's CAS checks for independent writers. The
[measured contention profile](evidence/machine-store-performance/manifest.json)
shows why: one winning call stays near a millisecond while competing stale calls
can wait much longer. These races target one predecessor of one machine;
independent-instance throughput and external execution are separate workloads.

## Shared-model example and evidence

Run the independent application with:

```sh
cargo run --manifest-path ci/machine-consumer/Cargo.toml --bin durable --features durable --locked
```

Its [source](../../ci/machine-consumer/durable.rs) imports the facade and explicit
optional SQLite adapter, and reuses the flat/timeline counter model and codec.
It prepares genesis, restores the owner, prepares one event, commits, verifies
and publishes the receipt, reopens and resolves an exact duplicate. After verified
admission it releases the local typed intent cache; durable bytes remain in the
journal. The same progression rejects overflow, inspects source provenance and
reopens both source/task databases after native admission but before source ACK.
Reclaim recovers the original task and refuses the stale lease. It then admits
a saga from a second event. All adapters share one monotonic host clock; resetting
it on reopen would violate the durable clock floor. It performs no external
effects in its default admission profile. The optional `protected` consumer
feature completes the native pure task through the existing Linux runtime and
commits only its verified output as the next machine event, retaining stable
result identity and task/source causation. That transition starts the saga.
[`run-supervised-saga-example.sh`](../../scripts/run-supervised-saga-example.sh)
builds the worker and checks both the successful round trip and refusal of a
completed worker with incorrect output. Stable and MSRV CI compile/run these
profiles. SQLite stays absent from the facade's normal dependency graph and
all portable profiles; no reverse adapter-to-facade dependency was added.

Evidence includes independent-worker CAS races, coherent concurrent readers,
wrong scope/predecessor, unauthorized reads/retries, frozen-range pagination,
quota refusal, silent-write rollback, substituted post-write bytes, deferred commit failure,
corrupted coordinate/payload refusal and
reopen/lost-ACK checks. Unix process-death tests send SIGKILL before transaction,
after begin, event insertion, intent insertion, head update, final validation and commit. Reopen
must expose the complete batch or none, and retry must resolve exact admission.
Migration process-death tests include the version-7 boundary while preserving
populated task/process rows and fences. A dedicated real-SQLite fuzzer compares
bounded operation sequences with an independent arithmetic/history oracle;
deterministic deep seeds exercise quotas, duplicates, history and reopen early.
See [raw logs and source hashes](evidence/machine-storage/manifest.json).

## Leased receiver handoff

`SqliteMachineOutbox` is an optional view of an `Arc<SqliteMachineStore>` and uses
that journal's bounded worker and connection. Construction pins a trusted
`MachineLeaseClock`, a `MachineIntentAcceptor` and its receiver namespace. The
outbox's `Receipt` is the configured acceptor's native receipt type. Callers
cannot replace the verifier on an acknowledgement. Host authorization covers
claim, validation, status and acknowledgement before any scoped reads or native
receipt lookup.

The first claim persists the configured consumer before returning any lease.
Validation, reclaim and acknowledgement require that same consumer, including
after reopen. Changing receiver namespace, workflow policy or input schema cannot
turn an uncertain delivery into a fresh downstream operation. `status.consumer`
reports the retained receiver; first-claim authorization also receives it.

A claim binds the full source scope/definition, committed revision, event, commit
identity, ordered intent metadata and immutable payload. It adds a distinct owner,
durable fencing token, expiry and attempt count. The trusted lease clock is separate
from captured machine logical time. SQLite persists a monotonic floor per machine
and rejects time regression and overflow. Claims use one immediate transaction,
respect count/byte bounds and revision/ordinal order, and never skip an oversized
first eligible intent. All leases are rechecked after writes and returned only
after commit. A shared checked fencing-counter helper also serves the existing
process outbox/timer protocol, while each protocol keeps its own typed token.

A lease is coordination evidence for source-authorized receiver admission. It does
not authorize arbitrary external execution. `requires_reconciliation()` is true
on every reclaim; expiry, reopen or an attempt limit never turns an uncertain
operation into an unattempted one. Receiver-specific idempotency, fencing,
reconciliation and execution authority remain mandatory. The generic outbox does
not execute or restart effects.

Acknowledgement checks the exact live source/lease before asking the pinned
acceptor to authenticate the native downstream receipt. The verifier must only
read/verify authoritative receiver acceptance, and must not submit or execute.
The worker then rechecks the source, payload, owner, token, attempt and expiry in
an immediate transaction before persisting the verified receiver identity and
receipt digest. A receipt verification delay cannot bypass lease expiry or a
concurrent reclaim. Exact retries return the stored acceptance even after expiry;
changed receipts/receivers conflict. `status` resolves lost acknowledgements
without re-executing an effect. This acceptance proves receiver admission, not
external completion. Structural DTO decoding and hashes do not authenticate
leases or receiver receipts.

Schema version 8 adds delivery rows and per-machine clock/usage columns without
rewriting version-7 event/intent wire bytes. Delivery metadata is charged once at
304 logical bytes per claimed intent, retained across reclaim/acknowledgement.
Journal commits and claims enforce the combined journal/delivery scope budget;
database pages bound physical row/index overhead separately. No history, dedupe
identity, uncertain attempt or accepted receipt is automatically discarded.

Focused tests include independent-worker claims, reopen/expiry/stale owners,
source/lease substitution, policy denial, receipt substitution, expiry during
verification, lost acknowledgements, attempt/token exhaustion, clock regression,
ordered byte bounds, whole-prefix quota rollback, suppressed/substituted writes
and deferred commit failure. Process-death tests cover eleven claim/ack boundaries
and five populated version-7 migration boundaries. The dedicated real-SQLite
outbox fuzzer compares bounded operation sequences to an independent lease,
expiry and acceptance oracle. Tests use explicitly configured receiver fixtures;
The production task and saga receivers below build on those source guarantees. Full
release qualification remains separate work. See [checkpoint evidence](evidence/machine-outbox/manifest.json).


Schema version 9 permits the consumer to be retained before acceptance, reusing
the existing metadata columns and logical charge. Migration preserves all source,
lease and accepted receipt bytes. Version-8 pending deliveries have no recorded
consumer: migration preserves that uncertainty, and claim/validate/ACK refuse
these unknown targets. Restore the original receiver and reconcile authoritative
acceptance under an explicitly reviewed recovery procedure; do not clear attempts
or assign a new receiver based on missing acknowledgement. Already accepted
version-8 receipts retain their consumer and exact acknowledgement behavior.

## Admit a recoverable task

`penelope::task::MachineTaskReceiver` accepts a committed machine intent into the
existing native task journal. Configure it with an `Arc<AsyncTaskJournal>`, a
host-owned receiver namespace, an exact task definition and an input schema.
Pin that receiver in `SqliteMachineOutbox`, claim a lease, call
`receiver.accept(&outbox, &context, &lease, task_tick)`, then pass its native
`TaskRecord` receipt to `outbox.acknowledge`. The
[durable consumer](../../ci/machine-consumer/durable.rs) demonstrates the complete
sequence using the facade and optional SQLite adapter.

Receiver identity binds its namespace, task definition fingerprint and input
schema. Native task identity binds that receiver and every source coordinate:
tenant, instance, machine definition/schemas, committed revision, event, commit,
intent identity, payload digest and byte length. Lease owner, token, expiry and
attempt do not alter native task identity. Source policy receives the configured
consumer during validation and acknowledgement, before native admission.

The receiver validates the live source lease before touching the task journal.
It proposes only the existing `IntentCommitted` event through `TaskProjection`;
sequence and generation are native zero values. The native journal validates and
commits the exact record and input bytes. An existing intent is read back with
its original tick. A competing admission resolves through authoritative lookup,
never a fabricated receipt. Source acknowledgement independently verifies the
exact native record and input blob, then rechecks the source lease before commit.

Lost acknowledgements may leave a durable task with a pending source intent.
Reclaim and repeat admission with the same source identity. Running, completed
or faulted tasks retain that original intent; delivery never adds a generation
or restarts execution. The host restores and drives `TaskSupervisor` separately,
under the task's original recovery permissions and reconciliation rules.
Acceptance proves durable task admission, not successful execution.

Machine and task journals may use separate databases. There is no transaction
across them: stable identity, native lookup and separately acknowledged acceptance
resolve the gap. Expired leases or unavailable/corrupt history leave source work
unacknowledged. Do not infer that a task was never admitted from a failed response.

`SqliteTaskJournal::into_async(queue_capacity)` moves the existing journal,
connection and validation cache to the shared bounded worker implementation.
It implements `AsyncTaskJournal` without another task evaluator. Construction and
migration are synchronous setup operations; subsequent native I/O runs on the
worker. A full queue refuses immediately. Dropping an accepted call does not
cancel its transaction; resolve a missing receipt by reading native history.
Input/output copies are bounded by the configured blob limit, and failed blob
reads leave the caller's buffer unchanged.


## Admit a saga

`penelope::saga::MachineSagaReceiver` starts a host-pinned linear or graph workflow
through the existing engines, `ProcessStore` and `ProcessPayloadStore`. Configure
a `MachineSagaDefinition`, authenticated service principal, native
`ProcessAuthorizer`, receiver namespace and source input bound. Source authorization
and native `Start` authorization both run before any native writes. The receiver
identity binds this exact policy; the source-bound outbox preserves it on reclaim.

Claim, call `receiver.accept(&outbox, &context, &lease, logical_time)`, then
acknowledge its `MachineSagaReceipt` through the source outbox. The
[durable consumer](../../ci/machine-consumer/durable.rs) uses one counter model,
recovers its state, admits a task and then admits a saga from a second event.
The example performs admission only and dispatches no external effect.

Each source/receiver pair has one deterministic native process scope, start input,
first action and outcome identities. The original engines produce the start and
exact outcome plan. Bootstrap atomically commits native outcomes and the first
action with `input: None`: no machine event is mislabeled as a canonical event,
timer, manual decision or task result. The native v1 event bytes and outcome
formats retain their existing meaning and replay through the existing engines.
No new saga evaluator or machine-shaped process journal is introduced.

The immutable source payload is retained in the derived native process scope as
admission context; the native start event is stored separately for replay. The
workflow definition supplies its step payloads and recovery policy. Source bytes
do not silently replace those commands. Applications can read the context by the
source digest exposed by the verified receipt.

Receipt verification uses the optional `ProcessCommitVerifier` port to check the
retained native commit marker, frontier, exact outcomes and first action. SQLite
shares the native commit-retry verifier under a read transaction; verification
never resubmits or repairs a batch. The receiver also checks original recorded
time, replay event and source bytes. Competing starts and lost acknowledgements
resolve by native lookup; later outcomes do not cause another start. Escalated
or otherwise advanced sagas keep their native retry, compensation, reconciliation
and manual-review authority. Source acknowledgement proves native admission,
not dispatch or effect completion. Native outbox workers handle subsequent work
through the existing authorization and idempotency protocol.
