# Changelog

All notable changes to Penelope are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and
this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
