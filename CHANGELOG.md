# Changelog

All notable changes to Penelope are documented here.

## 0.1.0 - 2026-09-15

Initial public release of Penelope, a typed foundation for reliable sagas and
long-running asynchronous workflows.

### Included

- Deterministic linear and graph saga decisions with ordered replay.
- Append-only outcome-log contracts with sequence and integrity validation.
- Typed identities, commands, events, effects, retries, timers,
  compensation, reconciliation, and manual review decisions.
- Backend-neutral ports for persistence, inbox/outbox, leases, timers,
  external effects, authorization, quotas, diagnostics, and review queues.
- StateChronicle correlation boundary for verified canonical events.
- No embedded DTO wire-version or schema-discriminator fields; transport
  compatibility remains an application-edge concern.
- Property, adversarial, fault-injection, replay, and fuzz coverage with
  Rust 1.85 MSRV and stable CI verification.
