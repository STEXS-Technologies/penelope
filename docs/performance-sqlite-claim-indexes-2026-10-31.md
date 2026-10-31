# SQLite pending-claim indexes — 2026-10-31

Schema version 5 adds two partial indexes so retained completed actions and
future timers do not force linear selection scans. The measured improvement is
in SQLite query work. No wall-clock percentage or overall application slowdown
is established by these instruction-count experiments.

Baseline source: `4f1a0f39cc84974d81aaf67c48f9ecee3db41fda`.
Candidate source: `b86a438`.

## Change and migration

- `process_actions_pending(scope, action_id) WHERE acknowledged=0` supports
  scope-filtered pending action selection in action-ID order.
- `process_timers_due(scope, due_at, action_id) WHERE state=0` supports
  scope-filtered due timer selection in due-time/action-ID order.

The existing SELECT queries, eligibility rules, result decoding, fencing,
expiry boundaries, token issuance, and transaction modes are unchanged.
Index creation and `PRAGMA user_version=5` run in the existing immediate
migration transaction. Errors roll back both the schema version and any index
created earlier in the transaction. Fresh databases and older supported schema
versions follow the same transactional path. Newer schemas remain rejected.

This is an additive schema migration, not a package-version change. Previous
adapters reject schema 5; deployment guidance continues to require backups and
closing competing writers before upgrade. Index construction consumes pages
and can fail under the configured database cap. Increasing capacity and
reopening retries the migration without modifying authoritative rows.

## SQLite-engine selection evidence

The new `claim_selection` benchmark uses the adapter's real WAL/FULL schema,
bundled SQLite 3.53.2, and the exact production SELECT text. Within the same
candidate binary, it removes the indexes for an unindexed control or creates
both for the indexed profile. This is not a comparison of two application
binaries. The verifier checks that both SELECT strings occur in the retained
production source.

Fixtures contain 16 eligible rows and 0, 1,024, or 16,384 additional rows for the
same scope. Additional outbox rows are acknowledged. Additional timer rows are
either completed or pending with due time 200, beyond the query time 100. All
16 selected IDs and their order must match in both index profiles.

The payload/checksum blobs are opaque synthetic bytes and the scope key is a
short synthetic BLOB. They are intentionally not decoded or passed through the
public claim ports. Instruction counts isolate SQL selection work and do not
include worker dispatch, domain validation, token updates, result allocation,
commit, or synchronization. Database page counts below are fixture-specific
and are not production capacity estimates.

| Additional rows | Timer population | Outbox VM steps, unindexed → indexed | Timer VM steps, unindexed → indexed | Timer sorts, unindexed → indexed |
| ---: | --- | ---: | ---: | ---: |
| 0 | Completed control | 235 → 202 | 413 → 206 | 1 → 0 |
| 0 | Future control | 235 → 202 | 413 → 206 | 1 → 0 |
| 1,024 | Completed | 5,355 → 202 | 5,533 → 206 | 1 → 0 |
| 1,024 | Future | 5,355 → 202 | 7,581 → 206 | 1 → 0 |
| 16,384 | Completed | 82,155 → 202 | 82,333 → 206 | 1 → 0 |
| 16,384 | Future | 82,155 → 202 | 115,101 → 206 | 1 → 0 |

EXPLAIN plans change from scanning the scope's primary-key index, with a
temporary ordering B-tree for timers, to the new partial indexes. Timer lookup
also applies the due-time bound in its index search. All recorded counters,
plans, selected identities, and page counts reproduced exactly in 20 isolated
release processes: 10 each on Rust 1.99.0 and 1.85.0, totaling 240 fixtures.
These are deterministic engine counters; no timing or statistical interval is
inferred from the repeated runs.

## Costs and limits

The benchmark also measures representative single-row engine mutations after
selection, using the same synthetic rows and SQL shape in each profile:

| Mutation | Unindexed VM steps | Indexed VM steps |
| --- | ---: | ---: |
| Outbox insert | 45 | 55 |
| Outbox acknowledgement | 26 | 38 |
| Outbox lease-only update | 31 | 31 |
| Timer insert | 38 | 49 |
| Timer acknowledgement | 26 | 39 |
| Timer lease-only update | 31 | 31 |

These are SQL-engine mutation controls, not full public write latency or exact
production-statement cost. They show added index maintenance rather than
assuming the read improvement is free. In the 16,384-row completed-history
fixture, combined database pages change from 3,156 to 3,158. With 16,384 pending
future timers, they change from 3,156 to 3,301. Future timers stay indexed until
completion/cancellation. Real key/payload sizes and page packing differ.

Pending but unexpired leases remain in the partial indexes and can still
require scanning. Indexes do not guarantee constant-time selection for every
queue distribution, eliminate write costs, implement history retention, or
establish improved end-to-end workload latency. The database page limit still
bounds admission and index construction.

## Public behavior and migration fault checks

Five new public-port integration tests cover:

1. Version 4→5 upgrade preserves a full logical snapshot of process rows,
   usage, counters, and live outbox/timer lease envelopes. The original leases
   remain valid for acknowledgement afterward.
2. A name collision during second-index creation rolls back the first index,
   leaves version 4 and process rows unchanged, and allows repair/retry.
3. An exact page cap with no free pages rejects index construction with
   `QuotaExceeded`, preserving version 4 and all process rows; increasing the
   cap permits upgrade and claiming the pending action.
4. Timer ordering by due time and ID, ties, limits, future exclusion, delivered
   and canceled exclusion, and inclusive lease expiry boundaries.
5. Outbox ordering, acknowledged exclusion, unexpired lease exclusion, and
   reclaiming only after expiry.

The existing real-process migration crash matrix now includes cuts after the
first index and after schema 5, in addition to seven existing cuts. These
SIGKILL tests start from version 1 with real task records, blobs, and generation
fence evidence. They verify that precommit death leaves version 1 with none of
the new tables/indexes committed, postcommit death leaves version 5, and
reopening preserves authoritative task bytes and fences. Version 4 live
process rows are covered by the separate upgrade and failure tests above.

The repeated campaign passed 70 isolated processes across both compilers:
250 public-port cases and 180 SIGKILL migration cuts. Full suites and targeted
runs provide additional checks beyond those repeated-campaign counts.

Passed with compiler wrappers cleared:

- Full workspace all-target/all-feature suites excluding `penelope-fuzz`, with
  `RUST_TEST_THREADS=1`, on stable and Rust 1.85.0.
- Workspace Clippy with all targets/features and `-D warnings`.
- Formatting and layer-boundary checks.
- `graphify update .`.

Each compiler passed 21 SQLite unit, 39 process integration, and 26 task
integration tests. The previous baseline's CI run `41160990082` passed.
Candidate CI is tracked separately after push.

## Retained evidence

[`verification-data/2026-10-31-sqlite-claim-indexes`](verification-data/2026-10-31-sqlite-claim-indexes)
contains engine and fault outputs, compiler/binary pins, exact SQL source,
source patch, build and full validation logs, scripts, and checksums. The initial
migration fixture failed to compile because rusqlite does not deserialize u64;
it was corrected to read u32 page counts and widen for capacity arithmetic.
That initial log is retained. No benchmark trials were discarded.

```sh
python3 docs/verification-data/2026-10-31-sqlite-claim-indexes/verify.py
```

The verifier checks hashes, query equivalence, complete compiler/repeat sets,
all instruction/sort/mutation counters, selected IDs, deterministic repetition,
fault-case counts, and SQLite suite results. Scripts retain original local
paths; executables are identified by hash and not stored in Git.
