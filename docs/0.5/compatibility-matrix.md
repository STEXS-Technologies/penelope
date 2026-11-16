# 0.5 compatibility obligations

Status: selected contracts qualified; final candidate qualification remains open.

| Existing surface | Preserve | Required evidence |
| --- | --- | --- |
| Facade root and `executor` imports | Existing 0.4 consumer source compiles | External legacy consumer fixture, unchanged after baseline |
| Linear and graph sagas | Decision/error precedence, bounded replay, compensation, action identity, unknown outcomes | Differential legacy and canonical API tests |
| Process v1 wire and commits | Exact existing decoding and encoding; saga-specific atomicity | Frozen goldens and process store conformance |
| Task history v1 and definitions | Record encoding, fingerprint, checkpoint and generation semantics | Existing wire/recovery models and cross-version replay |
| Task control/supervision | Journal-before-start, poisoning, fencing, reconciliation and quotas | Portable and hosted tests, subprocess crash profiles |
| Timeline metadata v1 | Independent namespace, correction/finality and exact-head rules | Frozen timeline goldens and property/crash gates |
| SQLite process/task stores | Existing data survives forward-only machine schema addition | Old database reopen, migration rollback and CAS tests |
| StateChronicle bridge | Exactly scoped canonical evidence; no inherited authority | Pinned upstream signed-event conformance |
| Default features | Timeline opt-in; no default runtime or database | Cargo feature matrix and external fixtures |
| Portable core | Approved Rust 1.89 minimum, no_std and allocation-free supported paths | MSRV, bare-metal builds and allocation probes |

## Canonical imports and preserved paths

The canonical paths expose the existing implementations. Machine, saga and task
preparation share `PreparedDecision` inspection; their publication, durable
admission and recovery authorities remain explicit:

| Current path | Target consumer path |
| --- | --- |
| Root saga exports, `executor::engine`, `executor::graph` | `penelope::saga` |
| `penelope::task` and root `TaskSupervisor` | `penelope::task`, including `TaskSupervisor` |
| `penelope::ports` | `penelope::storage` for storage contracts; retain non-storage contracts under their appropriate namespaces |
| `penelope::statechronicle` | `penelope::integration::statechronicle` |
| `penelope::timeline` | Same optional path |

The workspace MSRV increase from Rust 1.85 to 1.89 is an explicitly approved
source compatibility change for patched SQLite; see [verification](verification.md).

Retain existing exports through 0.5 unless a specific source change has a recorded
reason and migration path. New machine records use their own independently
versioned protocol. Do not convert old records to fake machine histories.
Downgrade support for newly admitted machine records is not established.

The baseline source manifest pins all existing tracked crate/test/script files.
Keep old wire fixtures independently checkable; a checksum inventory alone is
not a cross-version replay fixture or a complete public API compatibility test.

The unchanged import fixture and selected signature/exhaustive-match checks live
in [`ci/compat-0.4`](../../ci/compat-0.4/README.md). Its independently generated
0.4 corpus freezes 33,278 task/timeline byte, validation and replay observations.
The separate saga corpus freezes another 3,136 observations across both facade
paths. `scripts/check-saga-machine-parity.sh` checks all of them on current Rust
and Rust 1.89 without regenerating expected data from candidate libraries. These
bounded fixtures supplement existing wire and migration suites; they do not
prove every possible downstream program or deployment compatible.

The [static API audit](evidence/maintenance/public-api-audit.json) compares all ten
crates across 14 package/feature profiles against original 0.4 source, using each
revision's locked dependencies. Checked public export bindings remain intact;
local-item signature checks pass with a strict patch-level compatibility policy.
The signature checker misses a removed facade reexport, so an independent binding
inventory and the unchanged consumer verify that boundary. A separate public-field
removal is caught by the signature checker. Both controls and restored source
identities are retained. This host-only review supplements behavior, MSRV and
portable checks; it does not establish compatibility for every downstream program.

`python3 scripts/check-sqlite-upgrade.py` opens a frozen database written by the
actual `42ec187` adapters with their original locked dependencies. It verifies
all legacy table rows and schemas, exact task/process replay, payload bytes and
SQLite integrity through schema 6 → 9 and a second reopen. The fixture retains
an unacknowledged process lease; opening never dispatches or acknowledges it.
Current Rust and MSRV CI run this check. The
[producer identities](../../ci/sqlite-upgrade/baseline/manifest.json) and
[upgrade/rollback evidence](evidence/sqlite-upgrade/manifest.json) bound its scope.

For deployment, quiesce admission, workers and dispatchers and preserve a
consistent schema-6 backup with the matching registry before opening the new
adapter. Validate an isolated copy with dispatch disabled. Both original adapters
refuse the upgraded schema; do not change `user_version` or run mixed versions.
Rollback means separately restoring the original backup and prior deployment,
after establishing exclusive authority and reconciling any post-backup work.
Newly accepted work cannot be recovered by downgrading a schema or restoring an
older snapshot alone. Follow the [operating procedure](../reliable-execution-operations.md#upgrades-retention-and-backuprestore).
