# Changelog

All notable changes to Penelope are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and
this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- Reuse SQLite response waker registrations during repeated pending polls.
- Build Linux execution identities and checkpoint acknowledgements with fixed
  stack buffers. Checkpoint ACKs now allocate zero times after setup, verified
  with a real protected worker and a durably committed checkpoint.

### Fixed

- Reject suppressed required SQLite writes before acknowledging process/task
  commits, service updates, generation authority or head repair. Retirement
  verifies complete scoped history deletion before committing its tombstone.
- Clone and destroy SQLite response wakers outside completion locks, preserving
  ready results if executor callbacks unwind during registration.
- Linux worker responses at the maximum payload size now reject a trailing
  byte available in the same pipe read, matching smaller response handling.
  Each active response buffer reserves one additional byte; valid maximum-size
  completions and checkpoint acknowledgements retain their existing behavior.

## [0.3.0] - 2026-10-30

### Added

- Typed saga/task bindings, verified application output and independent effect
  evidence, committed dispatch acceptance, and stable task-result inbox relay.
  `TaskEffectResult` extends input categories; existing serialized categories and
  saga event/action formats retain their existing encoding.

- Optional Linux static single-process worker runtime with sealed executable
  identity, bounded IPC, seccomp restrictions, kernel CPU limits and durable
  generation fencing for the documented single-host deployment profile.

- Shared allocation-free task supervision through `TaskControl` and explicit
  caller-owned `TaskWorkspace`, with the existing hosted supervisor preserved.
- Optional SQLite task history/blob storage, persistent generation reservations
  and two-phase fencing, terminal-history retirement and maintenance operations.
- Optional SQLite atomic process commits, deduplicated inbox/source events,
  durable outbox and timer claims, lease renewal and fenced acknowledgements.
- Bounded worker isolation for async SQLite process ports, with explicit queue
  exhaustion and ambiguous cancellation semantics.
- Adapter tests for transaction-boundary process death, competing connections,
  lost acknowledgements, corruption, migration and stale leases.

### Compatibility

- The 0.3.0 release adds `ProcessInputKind::TaskEffectResult` and five
  `ProcessAuthorizationOperation` variants (`Dispatch`, `DriveTask`,
  `ReconcileEffect`, `DeliverTimer`, `RelayTaskResult`). Consumers with exhaustive
  matches must handle these variants when upgrading. Existing variant names,
  numeric discriminants and saga/task v1 record encodings are preserved.
- `penelope_executor::task::SupervisorError` remains available, including variant
  imports, exhaustive matches and error conversions, through a re-export of the
  shared core controller error.

### Fixed

- SQLite executor wake callbacks cannot terminate the database worker through
  unwinding, preserving committed results and draining other accepted requests
  after response cancellation or admission closure.

- SQLite outbox/timer claims fail and roll back if the durable lease-token
  counter update is ignored, preserving pending work and token allocation.

- Worker requests reject pinned payload bounds and schema zero before payload
  allocation or I/O, including when transport limits are looser.

- Linux worker polling drains observations to EOF before classifying child exit,
  preventing a queued valid completion from being reported as a task fault.

### Changed

- Larger linear definitions check duplicate IDs through bounded borrowed-ID
  sorting and strict reverse-order uniqueness, preserving zero allocations and
  exact admission errors. The measured 128-step fixtures use 82.76–98.39% less
  validation time; small-definition and existing-workload controls remain mixed.

- Bounded graph validation tracks transition keys with per-step outcome bits
  instead of a heap tree. Fixed confirmation measures about 11.9% fewer graph
  instructions, with timing variability and unchanged controls retained.

- Worker scope hashing uses a fixed stack buffer, removing its temporary heap
  allocation while preserving protocol bytes. The measured proposal-time gain
  is below 1%; detailed scope and counters are retained in the worker report.

- Canonical DTO encoding reserves per-type initial buffer capacity, and Serde
  identifier decoding admits borrowed text without a temporary String. Measured
  encoding and short/escaped-ID decoding improve; long-ID decoding has a retained
  4.03% timing regression. Wire bytes and typed admission remain compatible.

- Short hosted protocol identifiers store text inline; long values retain owned
  heap storage. Text admission, wire bytes, formatting, ordering and hashing stay
  compatible. The direct-task engine floor is about 51% lower in the measured
  short-ID fixture; its initial standalone DTO encoding comparison was 6.29% slower. Detailed profiles
  and the long-ID control are retained in the inline-identifier report.

- Linear saga completion clones only retained projection fields; action-history
  extension reserves capacity for the next identity. The direct-task benchmark
  measures an approximately 9% reduction in empty-task added engine time, with
  application overhead depending on task duration and integration services.

- Hosted task advance initializes its two dispatch buffers only for an actual
  start/restart via the shared controller's lazy `TaskDispatchWorkspace` provider.
- Hosted task advance supplies two dispatch buffers and polling supplies one
  proposal buffer to the same core controller. Operation-specific portable calls
  avoid borrowing unused buffers while preserving full-workspace admission.
- Portable workspace validation requires poll capacity for both checkpoint and
  completion proposals, including when checkpoint limits exceed output limits.

- Infrastructure dependency checks allow `rusqlite` only in the dedicated
  SQLite adapter; core, ports, executor and the facade remain independent.

### Qualification

- Full three-iteration public-boundary campaign: 48 runs of 100,000 inputs,
  plus real adapter crash/concurrency suites and stable/MSRV verification.
- Recorded SQLite, worker, hosted-memory and RISC-V stack evidence. Graph result
  cloning and definition-validation optimizations remove the original slowdown:
  a forty-pair comparison measures 15.24% lower graph median time than 0.2.0 and
  17.73% fewer instructions. Host-load limits and all other benchmark results
  remain disclosed; universal timing parity is not claimed. See
  [the qualification report](docs/verification-2026-10-30.md).

### Security

- Linux worker startup executes only the sealed CLOEXEC image descriptor through
  `execveat`. Ordinary `execve`, other executable descriptors and descriptor
  rebinding remain forbidden after startup. Real worker probes catch attempts
  to re-execute or load another executable under the retained seccomp filter.

## [0.2.0] - 2026-10-11

Penelope now provides recoverable task execution alongside deterministic saga
orchestration. Applications can declare recovery permissions, persist valid
checkpoints and reconcile ambiguous effects through runtime-independent APIs.
The existing saga engine remains independently useful, with its v1 history
representations and execution guarantees preserved.

### Added

- Recoverable task definitions with explicit restart-from-input,
  restart-from-checkpoint, reconcile-before-restart and never-restart policies.
- Checkpoint-aware execution with immutable input and result records, committed
  recovery boundaries and application-authorized checkpoint migration.
- Task supervision with generation fencing, typed fault observations, bounded
  recovery attempts, deadlines, restart delays and execution/resource budgets.
- Independently versioned task history and deterministic reconstruction without
  rerunning computation or repeating external effects during replay.
- Backend-neutral synchronous journal and runtime contracts, plus a bridge to
  existing saga effect execution.
- Optional StateChronicle verification evidence bound to the exact task scope,
  generation and checkpoint identity.
- An allocation-free `no_std` task core, optional portable Serde support,
  distinct identity/time newtypes and caller-owned bounded execution buffers.
- Recoverable computation examples and deployment contracts covering journal
  durability, runtime enforcement, checkpoint schemas and external-effect evidence.
- Recovery validation through process termination against filesystem and
  transactional SQLite test journals, concurrent supervisor stress, property
  testing and sanitizer-instrumented fuzz targets.

### Security

- Task scope, definition, generation, sequence and monotonic time are checked
  before execution history advances; stale or cross-scope results fail closed.
- Input commits precede execution, and checkpoints/results commit before
  acknowledgement. Ambiguous persistence outcomes require reconstruction.
- Live generations retain capacity for a fault or completion; rejected resource
  proposals fence execution before recovery proceeds.
- Corrupt checkpoints, incompatible schemas and unknown effects cannot become
  unsupported claims of completion or authorize blind retries.
- Runtime isolation, CPU enforcement, storage authenticity and the correctness
  of computation and physical side effects remain explicit application contracts.

### Changed

- The facade exposes both saga orchestration and recoverable task execution
  while preserving existing linear/graph decisions, retries, compensation and
  reconciliation semantics.
- Task definition fingerprints bind recovery permissions and resource budgets
  to execution scope, and strict task wire decoding rejects malformed records.
- The workspace supports Rust 1.85 and uses Rust 1.99 for development, with
  hosted adapters separated from the portable task core.
- Release verification checks all seven library packages; tag releases use the
  matching dated changelog entry as their GitHub release notes.

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
