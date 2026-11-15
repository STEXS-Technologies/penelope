# Accepted machine commit preparation

`penelope::storage::machine` adds hosted payload preparation around the existing
portable machine owner. It introduces no second transition interpreter and no
Clone, Serde or equality requirement on application state, events or effects.
This is preparation for a journal; the [machine store and SQLite snapshot profile](machine-storage.md) are now
implemented. Durable outbox delivery/reconciliation and downstream bridges remain
pending.

`MachineCodec<M>` pins complete deterministic payload encodings to the model's
full definition. `prepare_machine_commit` checks the expected scope/revision and
codec definition, encodes the predecessor, and encodes the same borrowed event
that it passes to `MemoryMachine::prepare`. Accepted candidates retain the
original exclusive preparation handle. Rejection and unhandled decisions retain
no commit batch. Encoding errors, invalid candidates, digest/charge mismatches,
capacity failures and unwinding leave the live owner unchanged.

The resulting `MachineCommitBatch` binds the canonical event, complete next-state
snapshot, predecessor/next-state digests, native provenance, optional causation
and every ordered intent payload to the independent machine wire record. Intent
hashes and charges are checked against actual encoded bytes. Model/codec purity,
completeness and determinism remain application obligations: hashing cannot
establish those semantic properties or authenticate the caller.

Callers supply bounded state, event and intent scratch. Each slice must cover its
configured maximum; state scratch is reused after hashing the predecessor.
Record, individual payload and total batch limits are checked, with checked
aggregate arithmetic before owned payload copies. The batch charge includes
metadata, event, next state and intent bytes; it is not application heap
accounting. This hosted path allocates retained payloads and an `Arc`; the pure
machine kernel remains allocation-free. Cloning the batch Arc shares immutable
bytes for asynchronous admission and acknowledgement-loss reconciliation.

`PreparedMachineCommit` implements the common read-only `PreparedDecision` view.
Its consuming `publish` checks the receipt's entire scoped new head, event identity
and complete commit identity before invoking the original local publication.
Both committed and already-committed receipts require the same binding. Wrong
receipts consume and abort local preparation; dropping a handle cannot undo an
already committed store transaction. Receipt DTOs are not self-authenticating:
applications must obtain them from an authorized trusted store or authoritative
lookup. A receipt for a committed batch does not assert that it remains the latest
head. Later writes still require authoritative predecessor CAS.

The native owner's pending typed intents are a local cache, not durable outbox
or dispatch authority. Recovery of pending effects and external delivery acknowledgement uncertainty
require the upcoming outbox and bridge protocols. Journal reads/writes now use
the explicit host authorization contract. An exact retained batch can support lost-ACK lookup; evaluating
the event again against a newer head cannot substitute for that lookup.

The `prepared_decisions` facade example uses the same counter model as the flat
and timeline examples, with an explicit canonical codec. It inspects the batch
then drops it, demonstrating inspection and abort without opening a journal. The
independent facade consumer compiles this exact source on stable and Rust 1.89.

Tests cover non-Clone state, receipt substitutions, both dispositions, causation,
zero-intent capacity, byte limits, codec errors/panics and unchanged local state.
A test-only journal peer exercises scoped CAS, predecessor digest verification,
exact duplicates versus conflicting batches, lost ACK and snapshot restoration.
It is not production storage or crash-durability evidence. A dedicated bounded
fuzzer compares preparation/publication against an independent outcome oracle at
capacities 0, 1 and 4. These checks do not complete F1, F6 or F8.

## Checked stored payloads

`MachineStoredCommit<INTENTS>::decode` validates an exact wire record and its
retained event, next-state and ordered intent bytes under explicit payload/batch
limits. It checks all hashes and charges before copying payloads and consumes at
most `INTENTS + 1` payload items, including from an unbounded source. Missing,
extra, reordered or substituted payloads fail. Empty batches support capacity 0.
The recovered object exposes read-only batch bytes and reconstructs exact
already-committed receipt coordinates. It cannot yield the accepted-batch Arc
used by a new preparation: structural recovery is distinct from model acceptance.

`restore` checks the caller's configured complete scope and pinned model/codec,
decodes the snapshot, requires byte-identical re-encoding, and invokes the native
owner's invariant-checked restoration. This snapshot path evaluates no event and
performs no dispatch. Local last-transition provenance and pending intents start
empty, as documented by `MemoryMachine::restore`; durable pending intent bytes
remain in the recovered batch for the upcoming authoritative outbox protocol.
It does not reconstruct earlier pending batches, replay history, verify the
predecessor chain, or authenticate the source. A trusted coherent journal read is
still required. The SQLite adapter supplies that transaction boundary; checked
recovery bytes alone do not establish durability.
