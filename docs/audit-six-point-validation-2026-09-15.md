# Penelope six-point library validation

This report covers only the reusable Penelope library. It does not claim that
an external database, broker, scheduler, KMS, worker pool, or StateChronicle
adapter is production-ready.

## 1. Atomicity and durable contracts

**Validated:** `AtomicProcessCommit` rejects cross-scope records, sequence
gaps, duplicate outcome identities, duplicate action identities, duplicate
effect keys, invalid schemas, oversized batches, and unbound canonical source
events. `atomic_commit_faults.rs` injects failure before input staging and
after each staged write; no partial state is visible and recovery is exactly
once in the test double.

**Command:** `cargo test --workspace --locked`.

**Boundary:** `ProcessStore`, inbox, and outbox durability are ports. A real
adapter must provide a transaction/CAS, uniqueness constraints, and crash
recovery; this repository intentionally ships no infrastructure implementation.

## 2. Recovery, timers, compensation, and ambiguity

**Validated:** deterministic replay, sequence validation, bounded logs,
retry-attempt monotonicity, deterministic backoff/jitter, inclusive deadlines,
early/duplicate timer rejection, LIFO compensation, cancellation, unknown
outcome escalation, and canonical reconciliation dispositions.

**Command:** `./scripts/run_pure_chaos_drill.sh` (three iterations).

**Result:** all 37 executor tests, restart/replay properties, timer checks, and
nine malformed-input fuzz targets passed in every iteration.

**Boundary:** durable timer claiming, inbox delivery, external consistency
windows, and restart persistence still require an adapter and fault-injected
integration suite.

## 3. Authorization, redaction, and tenant isolation

**Validated:** typed tenant/process/definition scope, fail-closed authorization,
distinct-principal requirements for review/terminal override, bounded quota
requests, redacted diagnostics, exact action/effect/resource correlation, and
cross-tenant rejection.

**Commands:** `cargo test --workspace --locked`; `fuzz_process_control_ports`;
`fuzz_statechronicle_correlation`.

**Boundary:** policy decisions, durable quota accounting, operator identity
proof, and access-controlled evidence retention belong to the composition root.

## 4. Definition evolution and migration safety

**Validated:** canonical definition digests, immutable registration receipts,
exact compatibility classification, migration identity binding, and source /
destination identity and digest checks. Changed semantics cannot masquerade as
an idempotent registration.

**Command:** `cargo test -p penelope-domain -p penelope-ports --locked`.

**Boundary:** applying a migration to running process state is intentionally a
consumer policy decision; no implicit state transformation exists in Penelope.

## 5. StateChronicle and external-effect safety

**Validated:** canonical command receipts bind exact action IDs; verified event
correlation binds tenant, action, operation, complete resource scope, and
payload digest; source-event deduplication requires an atomic inbox input;
unknown evidence never authorizes retry.

**Commands:** `cargo test -p penelope-statechronicle -p penelope-ports --locked`;
`fuzz_canonical_reconciliation`; `fuzz_statechronicle_correlation`.

**Boundary:** no client, network transport, durable inbox, canonical ledger,
or ambiguous-response adapter is included. A consumer must prove lost-response,
duplicate-command, delayed-event, rejection, and restart cases end to end.

## 6. Performance and resource safety

**Validated:** all public inputs are bounded; malformed-input fuzzing completed
without crashes or sanitizer markers. Pure host-local measurements were about
152k graph operations/s, 803 ns linear operations, 2.19M isolated parallel
graph operations/s, 16.9M isolated parallel linear operations/s, and 1.48 us
replay operations.

**Command:** `./scripts/run_pure_chaos_drill.sh` (includes release benchmarks).

**Boundary:** these are not shared-state, durable, network, or end-to-end
throughput figures. Storage contention, queue latency, worker scheduling,
backpressure, and p95/p99 service behavior require a consumer benchmark rig.

## Security and supply-chain gates

`check-layer-boundaries.sh`, `check-no-secrets.sh` (231 reachable commits,
zero leaks), `check-action-pins.sh`, `cargo audit`, `cargo deny`, strict Clippy,
formatting, rustdoc, and the full locked workspace test suite all passed on the
revision containing this report.

## Release conclusion

Penelope's pure deterministic engine and typed safety boundaries are validated.
It is not a durable production system until a separately owned adapter passes
the contract and recovery evidence listed above.
