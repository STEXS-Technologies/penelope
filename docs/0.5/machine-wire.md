# Machine commit metadata v1

The portable `penelope::machine` module now provides `MachineCommitHeader`,
`MachineCommitRecord<INTENTS>` and `MachineWireError`. This fixes the initial
accepted-batch metadata layout before adding a journal. The format is independent
of saga v1, task v1 and timeline v1; no existing decoder or authorization enum gains
a machine variant.

This is metadata, not a store, payload serializer, durable receipt or source of
authorization. `new` and `decode` establish structural validity only. [Accepted preparation](machine-preparation.md) binds payloads to a native candidate.
A durable store must additionally bind a real accepted candidate, authenticated scope,
canonical event/state/intent payloads and the corresponding local transaction.
Constructing a DTO does not establish any of those facts.

## Canonical layout

Fields appear in the following order. Integers are unsigned fixed-width little
endian. Byte identities/digests are exactly 32 bytes. No padding, text numbers,
implicit defaults, variable integers or trailing bytes are accepted.

| Field | Bytes |
| --- | ---: |
| Literal ASCII `penelope.machine.v1` | 19 |
| Accepted commit kind, exactly `1` | 1 |
| Machine tenant, instance, definition fingerprint | 96 |
| State schema and event schema, each u32 | 8 |
| Expected native revision, u64 | 8 |
| Accepted event identity and event payload digest | 64 |
| Captured logical time, u64 | 8 |
| Next native revision and transition code, each u64 | 16 |
| Stable event-type code within the pinned schema, u64 | 8 |
| Previous and next state payload digests | 64 |
| Causation presence flag, exactly `0` or `1` | 1 |
| If present: source namespace, occurrence, correlation | 96 |
| Intent count, u32 | 4 |
| Each ordered intent: identity, digest, u64 payload charge | 72 |
| Complete commit content identity | 32 |

The final identity is SHA-256 over **every preceding encoded byte**, including
the protocol tag, record kind, causation flag and intent count/order. It excludes
itself. Empty records are 329 bytes; causation adds 96 and each intent adds 72.
The caller's inline storage capacity is not encoded and does not affect identity.

Event/intent IDs, schemas, event/transition codes and logical times have no special
forbidden zero value in this format. Interpretation belongs to the pinned model
and codec. The next revision must be the exact checked successor of the expected
revision. A predecessor at u64::MAX is refused. Duplicate intent identities and
overflow in total payload charge are refused; a zero-charge intent is allowed.
Scoped store quotas will impose stricter limits at durable admission.

`MachineCausation` uses explicitly assigned machine-protocol source, occurrence
and correlation types. These are application assertions. They are not converted
automatically from saga action IDs, task generations or external canonical facts.
A downstream bridge must retain and verify its native source coordinates and
receipt independently; causal metadata never authorizes dispatch.

## Bounds and failure

`INTENTS` bounds inline storage and duplicate checks. Construction reads at most
`INTENTS + 1` items, including from an unbounded iterator. Decoding checks count
and exact remaining body length before entering an intent loop. Duplicate checks
are bounded quadratic in this configured capacity. Payload bytes are not read or
allocated by this codec. Applications choose capacities appropriate for their
stack and record limits.

Encoding checks the full destination length before changing any bytes and leaves
any suffix untouched. Decode refuses unknown versions/kinds, noncanonical flags,
truncation, trailing bytes, revision violations, duplicate/over-budget metadata
and mismatched commit identity. Errors never establish a partial record or commit.
Changes requiring a new interpretation need a new protocol version and explicit
migration. Do not regenerate v1 goldens to accommodate changed behavior.

The small fixed-field byte cursors are shared with the existing timeline codec;
record interpretation, errors and protocol framing remain separate. Existing
timeline goldens and fuzz checks verify this extraction.

## Payload and authority obligations

The event digest must bind the exact immutable encoded event bytes; the state
digests must bind complete predecessor and candidate snapshots under the pinned
state schema. Each intent digest/charge must bind its retained payload. The core
metadata codec cannot check these obligations because it receives no payloads.
No replay, snapshot restoration, delivery or I/O occurs during metadata decoding.

A journal must atomically bind the event, exact predecessor/new state and complete
ordered intent batch. An exact repeat of that batch can resolve a lost commit ACK;
the same event ID with a different batch must conflict. Re-evaluating an old event
against a newer head is not a substitute for authoritative committed-event lookup.
External effect ACK uncertainty remains a separate reconciliation problem.

A hash does not authenticate a caller, policy decision or store. An attacker who
can replace bytes and their hash can construct a different structurally valid
record. Store authenticity, authorized operations, exact receipt verification and
native downstream acceptance remain separate trust boundaries.

## Proof and remaining work

Two frozen fixtures are encoded independently using Python `struct`/`hashlib`,
without invoking Rust. They cover an empty batch, causation, ordered intents,
nontrivial endianness, u64::MAX revision/time/charge and a full-width event code.
Their bytes and identities are checked by Rust tests and
`python3 scripts/verify-machine-wire-fixtures.py` in CI. Tests also alter every
byte, truncate at every offset, recompute checksums over structurally invalid
records, exhaust bounds and verify untouched short output buffers.

The machine allocation executable now separately probes 10,000 commit
decode/construct/encode/refusal iterations with a positive allocator control.
The machine fuzzer covers raw bytes and mutated/rehashed fixtures; the explicit
newtype fuzzer covers every added identity/code type.

Hosted [canonical payload preparation and receipt matching](machine-preparation.md)
are implemented. [Authorized journal admission, SQLite transactions and snapshot recovery](machine-storage.md)
are also implemented. Durable outbox delivery and typed bridges remain pending.
This record codec alone does not qualify F1/F6/F8.
