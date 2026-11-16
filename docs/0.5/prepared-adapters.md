# Prepared saga and task decisions

The portable `PreparedDecision` inspection contract is now implemented by local
machine candidates, prepared linear/graph sagas and task record candidates. The
[public API](public-api.md) and compiling facade consumers exercise these views.
Inspection supplies no durable admission or execution authority; the
[SQLite journal and native bridges](machine-storage.md) provide those separate
contracts.

Saga preparation delegates to the original engines and returns their exact native
decisions. Linear preparation binds the source input and derives its replay event
when consumed, replacing separate event/decision construction in `trade`; the
compensation and retry examples now use the same preparation API. Graph
preparation preserves the existing event, scope, visit bounds and result routing.
The original fast functions, exhaustive error enums and wire formats remain intact.
No saga is reinterpreted as a general hierarchical application machine.

Task `propose`, `apply` and the new `prepare` share one private isolated-candidate
evaluator. Previously `propose` copied a candidate and then called `apply`, which
copied again. The new evaluator owns that validation once without routing legacy
application through the larger public view. Projection layout and transition
validation order are unchanged. Journal admission, fencing and runtime command
authority remain in the existing task control path.

## Differential and resource evidence

The independent saga consumer compares 3,136 observations against an actual
unmodified 0.4 checkout, then against both current legacy and prepared paths.
Every successful prefix is replayed. The corpus includes all six saga statuses,
compensation completion, timed retries, manual resolutions, unknown outcomes,
missing edges, invalid definitions and visit bounds. CI binds the source fixture
and old output hashes, preventing a simultaneous change to both current paths
from silently changing the expected observations.

Task parity compiles an exact frozen copy of the 0.4 projection. Sixty-four
mode/effect/seed profiles compare proposals, raw record validation, malformed
scope/version/order/generation/tick combinations, supervisory decisions, complete
private projection state and replay. Preparation/drop leaves the source untouched;
record transfer preserves the native v1 record. Existing recovery, receipt,
ambiguity, process-kill, fencing and allocation tests remain required.

The fixed-port RISC-V static task profile has the same conservative 115,056-byte
bound before and after, including its 65,552-byte positive-control branch. The
candidate reaches 43 functions versus 42 in the parent because evaluation now has
one shared helper. Allocated read-only ELF sections grow from 77,289 to 77,425
bytes in this profile; writable sections remain zero. These are profile-specific
measurements, not whole-application memory or arbitrary callback guarantees.
All five existing task lifecycle allocation profiles remain zero, and the local
machine probe retains its zero-allocation requirement and positive control.

The task-history and linear-engine fuzzers now inspect prepared candidates as
well as exercising legacy behavior. The task oracle above supplies an independent
old implementation; same-build fuzz comparisons additionally exercise the new
view and transfer paths. Fuzzing and differential tests complement each other.

The [durable consumer](../../ci/machine-consumer/durable.rs) composes machine
publication, recovery and native admission. [Timeline composition](timeline-machines.md)
uses the existing correction engine. Their evidence and remaining performance,
chaos and exact-revision release gates are tracked in [verification](verification.md).
