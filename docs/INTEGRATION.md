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
