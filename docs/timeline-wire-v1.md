# Timeline record protocol v1

`penelope_core::timeline::{TimelineRecord, TimelineIntentRecord}` encodes fixed-width
metadata into caller-owned bytes. Every record starts with the exact 20 ASCII bytes
`penelope.timeline.v1`, followed by one variant byte. This format is independent of
saga/task v1 and requires neither Serde nor allocation. There is no native-endian,
`usize`, string ID, padding, compression or implicit migration in the protocol.

All counters and byte charges are unsigned **u64 little-endian**. Digests and digest
identities occupy exactly 32 raw bytes. Each field occurs exactly once in its stated
position. Presence flags are a single byte: zero means absent and one means present;
other values are invalid. The encoder produces and the decoder requires precisely
one complete record, with no trailing bytes. The largest v1 record is 832 bytes.

## Field layouts

A `scope` is four consecutive 32-byte identities: tenant, timeline, model/code,
state/input schema. Scope is repeated in nested records to preserve exact bindings.
An `observation` is scope, authority identity, authority epoch, authority sequence,
observed logical step, observed digest (216 bytes).

A `head` is scope, branch, applied step, revision, finalized-presence flag,
optional finalized step, observation-presence flag, optional observation. It occupies
154, 162, 370 or 378 bytes depending on its optional fields. No finalized prefix is
distinct from a finalized prefix ending at step zero. Decoding checks finality is
at or before the applied step, and that an observation has the same scope and does
not refer to a future step. Observation acceptance never implies effect finality.

| Variant byte | Payload fields in exact order | Total bytes including tag/kind |
| --- | --- | --- |
| 1 — Checkpoint | head; highest consumed branch ID | 183–407 |
| 2 — Input | scope; branch; input step; stable input ID; input revision; payload digest | 237 |
| 3 — Anchor | scope; branch; post-input step; snapshot ID; complete state digest | 229 |
| 4 — Fork | expected head; new branch; anchor payload as above; replacement-presence flag; optional input payload as above | 392–832 |
| 5 — Intent | scope; origin branch; origin step; origin publication revision; stable effect ID; payload digest; payload byte charge | 245 |

A checkpoint's consumed branch ID must be at least its head's branch. Persist the
projection's highest reservation ID even if reservations were aborted: deriving it
from only the selected branch permits ABA after restart. Restore metadata with
`TimelineProjection::restore`, retaining authority/finality exactly as decoded.

A fork binds its anchor and optional replacement to the expected head's scope and
parent branch. Its new branch is greater than the parent's, its expected revision
must permit incrementing, and its corrected step is no later than the head and
strictly after any finalized prefix. A replacement input must immediately follow
the supplied post-step predecessor anchor. A state correction replays strictly
after its anchor. A fork record describes a candidate; it does not select it as live.

An intent's origin publication revision is captured in `StepContext` when live or
candidate steps execute. `EffectDescriptor::record()` produces this exact metadata.
Later head mutations do not rewrite intent origins. Encoding an intent grants no
dispatch eligibility, finality, durable admission or receipt of external delivery.

## Admission and recovery boundary

`decode` is a strict structural parser. It does **not** authenticate a source,
verify application payload bytes, admit a new model/schema, prove snapshot
completeness, check an entire journal sequence, deduplicate effects, or publish
state. In particular, a structurally valid replacement's revision must still be
compared with the original input by `ReplayPlan`; a new branch must still clear
the controller's full consumed-ID high-water mark. Reordered, stale or cross-scope
records must be refused by normal transition and trust admission.

A checkpoint is metadata, not a complete durable `LiveTimeline` recovery image.
A persistent adapter must atomically bind the head to the corresponding complete
state, retained inputs/snapshots, consumed IDs, pending intents, independently
authorized finality and authoritative effect receipts. Durable readers must not
combine a newly decoded head with older state. The provided in-memory publication
profile and this codec do not establish that persistence transaction automatically.

Payload codecs remain application-owned and pinned by model/schema identity.
They must check size before allocation, verify content against the descriptor and
retain all required dependencies. A digest is not authority. Existing v0.3 task and
saga bytes are unchanged. Unknown future timeline versions require an explicit
versioned migration; decoding never guesses compatibility.

Golden fixtures in `crates/penelope-core/tests/fixtures/timeline_v1/` were assembled
independently from these field layouts. Tests compare encoder bytes, decode each
fixture, exhaust all truncation positions and verify checkpoint authority/finality
and consumed-ID restoration. `fuzz_timeline_wire` checks canonical round trips for
raw input and mutated golden records and is registered in bounded/chaos runners.
