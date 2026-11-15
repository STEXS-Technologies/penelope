# 0.5 compatibility obligations

Status: requirements; no 0.5 compatibility qualification claimed.

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
| Portable core | Rust 1.85, no_std and allocation-free supported paths | MSRV, bare-metal builds and allocation probes |

## Planned canonical imports

These are target paths, not claims about APIs already available:

| Current path | Target consumer path |
| --- | --- |
| Root saga exports, `executor::engine`, `executor::graph` | `penelope::saga` |
| `penelope::task` and root `TaskSupervisor` | `penelope::task` with documented hosted composition |
| `penelope::ports` | `penelope::storage` for storage contracts; retain non-storage contracts under their appropriate namespaces |
| `penelope::statechronicle` | `penelope::integration::statechronicle` |
| `penelope::timeline` | Same optional path |

Retain existing exports through 0.5 unless a specific source change has a recorded
reason and migration path. New machine records use their own independently
versioned protocol. Do not convert old records to fake machine histories.
Downgrade support for newly admitted machine records is not established.

The baseline source manifest pins all existing tracked crate/test/script files.
Keep old wire fixtures independently checkable; a checksum inventory alone is
not a cross-version replay fixture or a complete public API compatibility test.
