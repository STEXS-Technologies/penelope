# Documentation

Start with [setup](../README.md#get-started) and [your first machine](getting-started.md).
The machine API is being prepared for 0.5; the published version remains 0.4.

## Guides

- [Public API map](0.5/public-api.md): machine, saga, task, storage and runtime entry points.
- [SQLite](../crates/penelope-sqlite/README.md): optional local persistence.
- [Linux process runtime](../crates/penelope-process-runtime/README.md): optional task workers.
- [Hosted integration](INTEGRATION.md): authorized delivery, timers and task results.
- [Reliable execution operations](reliable-execution-operations.md): recovery and reconciliation.
- [Timelines](timeline-examples.md): quickstart, correction and integration examples.

## Contracts and development

- [Release contract](RELEASE.md): supported guarantees, boundaries and validation commands.
- [0.5 plan and progress](0.5/README.md): active stabilization work and qualification gates.
- [Machine storage](0.5/machine-storage.md) and [wire format](0.5/machine-wire.md).
- [Timeline architecture](timeline-foundation.md) and [wire format](timeline-wire-v1.md).
- [Security policy](../SECURITY.md) and [response procedure](security-response-rehearsal.md).

Dated investigation reports were removed from the working tree. Citations to those
reports link to immutable Git history. Raw validation data remains in
`verification-data/` and `0.5/evidence/`; it records the revision and scope of each
run and does not imply that the current branch has passed final qualification.
