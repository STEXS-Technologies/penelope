# Hosted integration services

The `penelope_executor::services` module provides bounded calls through injected
ports. The host owns the worker loop, authenticated principals, clock basis and
policy configuration. These calls do not create a scheduler. Optional SQLite and
Linux adapters remain outside the executor and the default facade.

`claim_outbox` authorizes `Dispatch` for the exact claim scope and owner before
claiming one bounded batch. It validates scope, ownership, expiry, pending state,
batch size and duplicate leases before returning any work. Supply trusted saga
logical time. Deliver only committed actions using their stable identity, and
acknowledge a lease only after validating durable downstream acceptance. Failed
or ambiguous delivery requires reconstruction rather than assuming rollback.

`dispatch_outbox` combines an authorized claim with bounded idempotent delivery.
It checks fresh trusted time before each dispatch, validates the dispatch receipt
against the exact action and complete scope, then acknowledges with a fresh clock
reading. Receipt mismatch or expiry leaves the lease unacknowledged, even when
work may have reached the dispatcher. Its report counts verified downstream
acceptance separately from outbox acknowledgements. Reconcile/redeliver through
the same stable action identity. Dispatch acceptance does not mean the action's
application effect succeeded.

`drive_task_once` requires a validated `CommittedTaskEffect`, matches the task's
scope and definition, authorizes `DriveTask` and makes one transition or poll with
an injected `TaskClock`. The host controls how often it calls this function.
Journal/runtime errors preserve the supervisor's normal reconstruction rule.
Rebuild the saga from authoritative events to find an active task whose outbox
was already acknowledged, then use `CommittedTaskEffect::recover`; this path
requires an existing durable task intent. New intent requires leased acceptance.

`reconcile_effect` authorizes `ReconcileEffect`, queries the injected external
effect port and validates its exact action/effect coordinates. It never executes
or cancels an effect. Unknown remains unknown and requires further evidence or
operator handling. It cannot authorize an automatic retry or successful result.

`drain_timers` authorizes `DeliverTimer`, reads the injected saga clock and claims
at most the request limit. It replaces the request's timestamp with trusted time.
All returned leases are validated before delivery. For each timer it persists
immutable schedule bytes and accepts a stable `TimerFired` inbox identity, checks
both receipts, then acknowledges with a fresh clock reading. Lease tokens,
attempt counts and delivery time do not change the input identity. Lost ACKs
therefore redeliver the same immutable input. A changed schedule under the same
action conflicts instead of producing a second input.

Timer inbox acceptance queues durable work; it does not prove that a saga
handled the firing. The consuming application must read accepted timer payloads,
construct the existing typed retry-timer event, validate the engine's outcome
plan, atomically commit the accepted input/outcomes/actions and validate the
receipt. Keep the schedule payload until replay/retention requirements permit
its removal. Expired leases may redeliver; they never prove that an effect failed.

`relay_task_result` authorizes `RelayTaskResult` for the verified binding's exact
scope and authenticated caller before committing the stable result input through
`ProcessStore`. The relay checks input identity, payload digest, definition scope
and receipt. The consuming application still validates the engine's outcome plan
and persists referenced replay payloads before the result commit. A denied call
cannot advance the process log; a lost receipt is recovered with the exact batch.

Timer failures return `TimerDrainError` with the stage, coarse port error and
partial claimed/acknowledged/duplicate counts. An inbox write may have committed
even when an error is returned. Retain evidence, allow lease recovery and repeat
the same delivery identity. Map these errors to the existing redacted diagnostic
port without logging payloads or raw credentials.

Authorization uses dedicated `Dispatch`, `DriveTask`, `ReconcileEffect`,
`DeliverTimer` and `RelayTaskResult` operation categories. Existing start, cancel,
retry and review permissions do not implicitly grant these categories. The policy
adapter must fail closed for permissions it has not configured. A principal must
come from the host authentication boundary; code/content digests and fencing
tokens do not authenticate a caller. Low-level ports and composition functions
are protocol building blocks and require authorization in their outer boundary.

## Optional checkpoint state evidence

Applications using StateChronicle can inject a
`penelope_statechronicle::task::CheckpointStateVerifier` and call
`verify_checkpoint_state(record, verifier)`. The combined path rejects unsupported
or non-checkpoint records, invokes the verifier, then binds its returned evidence
to every task-scope coordinate, generation and checkpoint digest/length/schema.
It propagates verification failure and rejects substituted evidence. The verifier
must establish proof authenticity, trusted key policy and the authoritative state
version; Penelope's correlation checks do not establish those facts themselves.

If independent evidence must gate checkpoint acceptance, place this call in the
application's journal/commit decorator before returning a successful commit
receipt to the task controller. A later audit check does not retroactively gate
the worker's checkpoint ACK. Persist and retry immutable evidence independently;
there is no transaction spanning StateChronicle and the task journal. The existing
`bind_checkpoint_state` remains available for applications that already invoked
their verifier. Pure task durability does not require either integration path.

The protected-worker integration test in
`crates/penelope-process-runtime/tests/support/checkpoint_state.rs` demonstrates
this decorator with SQLite: rejected verification or substituted evidence leaves
the worker paused without an ACK, and reconstruction fences that generation. Its
trusted verifier is a fixture; applications must supply actual proof verification.

The full linear and graph saga fixtures in
`crates/penelope-process-runtime/tests/support/sagas.rs` also exercise
`ReconcileRequired` tasks against shared and separate SQLite stores. They fence an
interrupted worker, persist an unknown outcome, reconstruct the blocked state,
then supply explicit trusted restart authorization. A successor's completion
still requires matching independent effect evidence before the normal authorized
result relay can commit a terminal saga decision. These external evidence sources
are fixtures, not live external systems or StateChronicle proof verification. The
coordinator crash suite covers 56 effectful cases (two engines, two store layouts,
14 boundaries), including initial launch, reconciliation required, unknown evidence
and persisted restart authorization. Each case uses SIGKILL followed by a fresh
process; reconciliation cases assert the original generation and blocked or
authorized state before recovery continues.

## Discovering accepted inputs after restart

`penelope_ports::inbox::PendingInbox` exposes read-only, bounded discovery of
inputs accepted by `Inbox` but not consumed by `AtomicProcessCommit`. SQLite
implements it on the same blocking-isolation worker as the other process ports.
Call `pending_inputs(tenant, process, limit)` with a nonzero limit up to 64.
Results are ordered by input ID; this ordering does not establish causality or
FIFO delivery. Reads do not acquire leases, acknowledge timers, or consume inputs.

The host authorizes access, selects its pinned definition, reads the immutable
payload through `ProcessPayloadStore`, and validates its complete scope before
mapping it to a saga input. In particular, a `TimerFired` input's timer payload
must match the expected action, attempt and due time; tenant/process discovery
alone does not establish that definition binding. Replay the authoritative saga,
decide, persist event bytes, then commit that exact accepted input with the
engine's outcomes/actions. Only the successful atomic commit consumes it.
Concurrent hosts may discover the same input; CAS and immutable input deduplication
arbitrate the commit. A timer delivery ACK still leaves its input discoverable
until the saga decision commits. Retrying discovery after a read error must not
turn missing or corrupt evidence into an empty inbox.

The `discovered_timer_commits_one_retry_task_and_replays_terminal_saga` runtime
integration test follows this complete linear-saga path with shared and separate
SQLite databases. It reopens after timer acceptance, consumes the discovered input
through the engine's outcome plan, executes the retry in a protected worker,
relays the verified result, and checks that repeating the timer commit leaves
no action available for redispatch.

The companion `discovered_graph_timer_selects_one_retry_task_and_replays_terminal_state`
test uses an explicit graph timer step between failure and retry. Its application
payload pins the timer's due time; the host verifies that payload, complete scope,
active action and accepted delivery before submitting the normal graph result.
Both database layouts reconstruct the graph and complete the verified retry task
without adding new graph inputs or action kinds.

## Executor composition example

Run `./scripts/run-executor-saga-example.sh`, or keep its state directory with:

```sh
cargo run -p penelope-executor --example saga_recoverable_task -- STATE_DIRECTORY
```

This example uses separate SQLite process/task stores and a simulated in-process
runtime. It commits the saga action before task acceptance, interrupts the
coordinator after a durable checkpoint, reconstructs and resumes that checkpoint,
verifies the output, and commits the stable result through the authorized relay.
The next invocation replays completion without dispatching another task.

The simulation provides no OS isolation or independent budget enforcement. Its
fixed demo principal illustrates scoped authorization; hosted applications must
inject an authenticated principal and policy. SQLite and the example's policy
helper are development dependencies of the executor, so normal executor users
do not acquire a storage backend. Use the Linux reference below for protected
worker execution.

## Executable Linux reference

Run `./scripts/run-supervised-saga-example.sh` on the supported Linux x86-64
profile. It compiles a freestanding static test worker with `cc`, runs the
`saga_recoverable_task` example with separate task/process SQLite databases,
and runs it again to verify terminal replay without another dispatch. It
requires `cc`, `rg` and the runtime kernel facilities documented in
[the runtime profile](../crates/penelope-process-runtime/README.md).

For retained state, compile the fixture as shown in that script, then run:

```sh
cargo run -p penelope-process-runtime --example saga_recoverable_task --locked -- STATE_DIRECTORY STATIC_WORKER
```

The state directory must belong to the current OS user. The CLI maps that
kernel-authenticated UID to a local principal and allows only this example's
scope and required operations. This is a local demonstration policy, not an
HTTP authentication service or a general tenant authorization policy.

The example writes event payloads before outcome commits, validates commit and
lease receipts, accepts/reconstructs durable task intent, drives the real Linux
runtime, classifies the exact sum output, commits the stable result inbox input
and replays the saga from its stored event bytes. The worker executable is
independently sealed/pinned by the runtime. Changed worker code or output cannot
silently resume the old binding.

Keep both databases and the registry when interrupted. A live old generation
must be fenced before restart. If a crash leaves an unexpired claim before any
task intent was accepted, the immediate rerun fails closed; retry after lease
expiry instead of deleting evidence. Already accepted tasks can recover even
when their outbox delivery was acknowledged. Unknown/blocked states require
independent evidence or operator handling.

Task ticks in this example use an injected monotonic millisecond clock whose
new origin starts beyond the persisted task frontier. Saga leases use a separate
host wall-clock basis. Production deployments must choose and document their
trusted clock/reset policy; neither task ticks nor this CLI imply a game-frame
or calendar deadline. The example's worker-driving loop is bounded, and runtime
CPU enforcement remains independent of polling.
