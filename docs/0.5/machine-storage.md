# Scoped machine journal and local SQLite profile

The hosted `penelope::storage::machine::MachineStore<INTENTS>` now has a concrete
`penelope_sqlite::SqliteMachineStore<Policy, INTENTS>` implementation. It stores
canonical accepted batches from the native owner, not a second machine evaluator.
This is the initial journal/snapshot profile. Intent leasing, source-authorized
downstream dispatch, delivery acknowledgement/reconciliation and typed saga/task
bridges remain open; this checkpoint does not complete F6 or release qualification.

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
durable intent history. Pending-outbox recovery and external delivery state are
not implemented by this snapshot helper.

Payload limits bound each record, state, event, intent and complete batch. Shared
SQLite limits additionally cap machine scopes, events per machine, logical retained
bytes and database pages. Logical scope charge is genesis plus current head-state
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
journal. It performs no external effects. Stable and MSRV CI compile/run this
exact source. SQLite stays absent from the facade's normal dependency graph and
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
