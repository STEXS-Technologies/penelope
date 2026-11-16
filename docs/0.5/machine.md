# Portable machine contract

The portable implementation is available through `penelope::machine` and
`penelope_core::machine`. Optional [durability and native task/saga admission](machine-storage.md)
and [timeline composition](timeline-machines.md) build on this contract.
Final 0.5 release qualification remains open.

Implement `Machine` with application-owned state, event, effect and error types.
Evaluation borrows current state and returns an owned candidate. `MemoryMachine`
checks scope, definition and predecessor revision, bounds staged intents, validates
the candidate and returns an exclusive, consuming prepared handle. Drop the handle
to abort, or publish it to replace state, revision, provenance and pending intents
together. Preparation never mutates the owner's published tuple. Model code must
provide independent state ownership and deterministic, side-effect-free evaluation;
Rust cannot prevent interior aliases or I/O inside consumer implementations.

Intent limits apply to the entire retained pending batch. Duplicate identities,
count/byte overflow and ignored staging failures abort the attempt. Intents retain
publication order and their originating transition. `take_intents` transfers them
to the application; it supplies no durable admission or delivery guarantee. Event
IDs and digests are caller assertions here, without historical deduplication,
canonical byte verification or authentication. Snapshot adoption is likewise an
application trust boundary, not journal recovery.

`Hierarchical<M, DEPTH>` implements the same trait and uses the same owner. Guards
walk the active leaf toward its parents; only `Unhandled` bubbles. Guards receive
no sink. External transitions exit the active leaf toward the common ancestor,
run the action, then enter toward the target. An external self-edge exits and
reenters the leaf; internal transitions retain that leaf. Parent selection does
not change the exit domain. Cycles, exhausted depth and candidate/target mismatch
fail closed. A hook error or staging poison stops subsequent hooks and discards
the candidate. There are no parallel regions or implicit completion loops.

The examples `counter_machine` and `nested_order` compile unchanged in the
independent `ci/machine-consumer` package, which depends only on the facade.
The nested enum example covers parent fallback, rejection and intent staging.
Neither example requires Clone, a state registration map or a serializer.

Validation includes generated transition tables and parent forests, flat arithmetic
and exhaustive seven-node tree oracles,
guard and hook failures, abort, stale heads, revision/byte/count bounds, duplicate
intents and destructor panic boundaries. Compile-fail examples cover identity
separation and exclusive consuming handles. The allocator probe has a positive
control and observes zero allocations across 10,000 flat and 10,000 hierarchical
operations. A bounded fuzz target checks decisions, retained intents and revision
against an independent reference model at capacities zero, one and four.

The metadata admission preflight is [shared with timeline](staging-extraction.md),
with differential and performance evidence. Storage, provenance, poisoning and
finality remain owned by their respective engines. [Flat and hierarchical
measurements](verification.md#flat-machine-measurement-fixture) cover dispatch,
state sizes, guards and intent quotas. [Current qualification](verification.md)
records passing coverage and package checks, with full chaos, retained performance
findings and final release reconciliation still open.
