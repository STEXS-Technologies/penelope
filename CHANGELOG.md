# Changelog

All notable changes to Penelope are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and
this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-10-10

Prepared release; tagging and registry publication are separate actions.

### Added

- Pull request template with crate/API impact, compatibility, documentation
  and validation checks for saga and recoverable task changes.
- Independent recoverable-task contracts and bounded deterministic task history
  replay, without changing the saga v1 protocol or decision engines.
- Explicit restart modes, immutable input/results, committed checkpoints,
  task generations, typed faults, deadlines, delays and resource quotas.
- Injected journal/runtime ports and a fail-closed supervisor that commits
  before dispatching and reconstructs after ambiguous acknowledgements.
- Explicit checkpoint migration and independent external-effect reconciliation.
- Optional StateChronicle checkpoint-state correlation and saga effect bridge.
- Allocation-free no_std core with optional no_std Serde and cross-target CI.
- Real process-kill persistence/effect tests, seeded recovery chaos, task wire
  fixture, and task history/checkpoint/supervisor fuzzers, including ambiguous
  persistence acknowledgements and storage faults.
- Concurrent supervisor arbitration under 32-contender thread stress and
  reproducible property/fuzz campaign seeds verified by runner tests.
- Timed per-property stress campaigns with rotating seeds and measured
  duration, batch counts and case counts saved outside the repository.
- Distinct task identity, generation, sequence and timestamp newtypes with
  transparent wire encodings, direct newtype fuzzing and AST-based architecture
  checks for fuzz coverage and declaration-only module entry files.

### Security

- Reserve history capacity for live-generation termination; fence and require
  reconstruction after rejected live quota proposals without checkpoint ack,
  including oversized reports with truncated buffers.
- Enforce deadlines on forwarded observations and document persistent fencing
  against delayed dispatch, with real-thread CAS/fencing contract tests.
- Reject AWS SDK package families in the architecture dependency gate, with a
  black-box dependency check in CI.
- Forward persisted input codec versions to runtime admission so unsupported
  schemas can reject before computation.

### Changed

- Verify library packages in isolated temporary registries, exclude the fuzz
  crate from packaging, and retain verified archives.
- Domain-separated definition fingerprints bind all recovery permissions and
  quotas to execution scope; golden fixtures freeze their canonical encoding.
- Strict new task wire decoding rejects unknown and duplicate fields without
  changing existing saga v1 representations.
- Admission bounds precede hashing, monotonic clocks precede polling, and
  saturating timeout arithmetic preserves enforcement near tick overflow.
- Corrupt journal/blob reads, stale generations and ambiguous adapter receipts
  fail closed. Trusted adapters retain responsibility for authentication,
  durable storage, fencing and independently verified effect evidence.

## [0.1.0] - 2026-09-15

Initial release of Penelope, a foundation for reliable sagas and long-running
asynchronous workflows. It provides deterministic decisions and recovery
contracts while leaving storage, messaging, scheduling, and service adapters
to the consuming application.

### Added

- Deterministic linear and graph saga orchestration with ordered replay after
  restart.
- Append-only outcome-log contracts that reject gaps, duplicates, reordering,
  and cross-process records.
- Reliable retry handling with bounded attempts, backoff, jitter, deadlines,
  and durable timer decisions.
- LIFO compensation, cancellation, unknown-outcome escalation, and manual
  review flows for exceptional or ambiguous work.
- Reconciliation contracts that distinguish committed, not-committed, and
  unknown external outcomes without blind retries.
- Typed ports for persistence, inbox/outbox delivery, leases, timers, external
  effects, authorization, quotas, diagnostics, and review queues.
- StateChronicle integration boundary for idempotent commands and verified
  committed-event correlation.
- Developer-facing constructors, facade exports, examples, documentation,
  and release verification for building reliable async systems.

### Security

- Tenant, process, definition, action, effect, and resource scopes are checked
  before decisions advance.
- Stale, duplicate, forged, or cross-scope results fail closed.
- Ambiguous external effects escalate for reconciliation or operator review
  instead of being treated as failures.
