# Machines in the optional timeline

`penelope::timeline::MachineTimeline` implements the existing
`RewindableComputation` for a pure `Machine`, including `Hierarchical` models.
Enable the facade's `timeline` feature. Run the shared counter example with:

```sh
cargo run -p penelope --example timeline_machine --features timeline --locked
```

`counter_machine` and `timeline_machine` use the same model source. The first
publishes through `MemoryMachine`; the second chooses `LiveTimeline` as its owner.
The timeline composition does not place a memory publisher inside a timeline or
maintain a second journal. Existing reservations, replay, head fencing, candidate
validation, finality and handoff retain their current implementations.

## Capture and pinning

Construction binds a machine scope to an explicitly supplied timeline scope.
Their tenant, instance, model, schema and revision types remain distinct. The
binding does not authenticate either scope. Each evaluation checks both domains,
the expected native machine head and the model's pinned definition/schema.

`MachineTimelineInput` owns the event and full `TransitionContext`: expected
machine head, event identity, event digest and logical time. Replay passes that
captured context unchanged to the machine. It never synthesizes machine revisions
or logical time from a new timeline branch or revision. A corrected snapshot must
preserve the native head expected by its retained suffix; otherwise replay fails.

Application events must capture every other dependency used by their computation,
including configuration versions, lookup results and random values. Snapshots
must contain complete state and retain no mutable aliases. Implementing `Machine`
alone does not establish these properties or make a model eligible for rewind.

Change the definition fingerprint or either machine schema by establishing a new
consistency domain and an application-defined migration boundary. This adapter
refuses mismatches; it does not reinterpret an old replay window under new code.

## State, effects and failure

`MachineTimelineState` contains the native head, application state and last
accepted transition record. `restore` adopts a trusted snapshot with no asserted
last record, as the native memory restore does. Complete in-memory history clones
retain the record. No `Clone` bound is imposed by the adapter: callers can use
`prepare_live_step` with their own complete isolated restore implementation.
The `apply_input` convenience and `CapturedSnapshot` profile require `Clone`.

Accepted candidates pass model validation and checked native revision advance.
Rejected and unhandled events are concrete computation failures for this profile;
they abort a live append or the entire correction. They are not recorded as
accepted no-op steps. Application evaluation, validation and staging failures also
leave live publication unchanged.

`MachineTimelineIntent` preserves captured machine origin and native proposed
intent metadata/payload. The timeline descriptor explicitly assigns the native
intent ID's bytes to its own effect-ID role and retains digest and payload charge.
The native charge remains a declared payload charge, not wrapper size or measured
heap use. Timeline origin remains separate and changes with corrected branch and
revision. No `CommittedIntent` is manufactured. Intents remain tentative until the
existing independently finalized prefix and `EffectHandoff` authorize admission.
Receipt validation, durable idempotency and external-effect success keep their
existing application boundaries.

The bridge pre-poisons staging before transferring a payload. An ignored error,
or a caught panic in a rejected payload's destructor, cannot produce a successful
step. Within-step failures stop hierarchy hooks via `IntentSink::status`.
Duplicates/quotas against earlier retained effects are checked by the existing
timeline publication boundary, after candidate computation. This intentionally
retains timeline ordering rather than duplicating its pending batch in the
machine adapter. Error precedence for a failing model and a retained-batch
publication conflict can therefore differ from `MemoryMachine`.

`HistoryPayload` implementations for the wrappers bind all machine context,
state/provenance and application payload digests with separate versioned hash
domains. They are in-memory content identities, not durable machine wire codecs.
Underlying payload implementations must bind all relevant bytes and report honest
charges. Caller-supplied event and intent digests still require independent binding
and authentication where an application needs them.

## Evidence and remaining work

The adapter tests cover 2,048 native-memory/timeline event attempts, failure
isolation, independent machine/timeline revisions, complete non-Clone restore,
all captured context roles, panic poisoning, hierarchical correction, actual
`FixedHistory` replay, tentative effects, finalized handoff and irreversible-prefix
refusal. The machine fuzzer now also checks the adapter against an independent
vector reference at capacities zero, one and four. Existing timeline property and
integration tests remain in place.

The adapter supplies in-memory correction; [machine durability](machine-storage.md)
is a separate profile and does not persist the full timeline. Current coverage,
package checks and remaining chaos, performance and release gates are recorded
in [verification](verification.md).
