# Atomic input admission workload option, 2026-10-31

The opt-in atomic-admission integration reduces durable workflow latency by 21.52–22.91% in two measured campaigns. It uses the existing `AtomicProcessCommit` input contract to avoid a separate `Inbox::accept` transaction when no independently durable ingress acknowledgement is required. Production library code and SQLite WAL/FULL settings are unchanged. The original preaccepted reference path remains the default.

This is a choice of acknowledgement boundary. Both modes leave identical successful durable state, but they deliberately differ before success: preacceptance retains a pending input after a later decision commit fails; atomic admission rolls that input back with the failed decision commit. Callers must retain/redeliver unacknowledged inputs in atomic mode. The performance result must not be used to erase an application's independent ingress durability requirement.

The checked eight-step workflow has nine input-bearing decision commits. Atomic mode removes nine separate acceptance transactions and worker roundtrips; payload writes, atomic commits, outbox leases/acks, result verification and replay remain. The benchmark validates the expected fresh-commit receipt flag in each mode. Production callers must also handle valid duplicate commit receipts on retries.

## Results

| Campaign | Path | Preaccepted ms/workflow | Atomic ms/workflow | Change | Block-bootstrap 95% interval |
| --- | --- | ---: | ---: | ---: | --- |
| 1 | durable | 47.282 | 37.105 | -21.525% | [-27.176%, -15.844%] |
| 1 | durable_replay | 49.925 | 37.591 | -24.706% | [-32.333%, -18.516%] |
| 2 | durable | 47.881 | 36.913 | -22.907% | [-30.998%, -10.320%] |
| 2 | durable_replay | 49.158 | 41.861 | -14.846% | [-21.571%, -7.842%] |

Each comparison uses the same executable, business kernel, work size, scopes, database configuration and post-completion checks. Negative means faster. Ordinary timings include complete managed workflows, not instrumented allocation probes. They do not measure cold database creation/migration, ingress network acknowledgement or a real process crash. Midpoint clean reopen restores both the projection and business value from verified payloads. The remaining approximately 37 ms durable cost is still large beside this fixture's approximately 0.48 ms of computation; no universal low-overhead claim follows.

## Method

Baseline source parent: `5d826b94a747278f4b5aa446d46b781dcecd7452`. Exact source and executable hashes are in metadata. Two campaigns, six balanced ABBA/BAAB blocks per campaign, four processes per block and two durable cases give 96 processes. Each process warms one workflow, then measures two alternating direct/managed rounds of four workflows: 192 paired timing rows, 768 measured managed workflows, 384 per admission mode. Direct computation checksums match all managed outputs. The direct fields are retained but are not the denominator of this admission comparison: its ratio is atomic managed time / preaccepted managed time.

Within each block, geometric mean managed time per workflow is computed for each mode and their log ratio is compared. Twenty thousand deterministic block-bootstrap samples produce the reported unadjusted per-profile intervals. All four intervals exclude zero. CPU 0, Ryzen 7950X, Linux, project NVMe btrfs storage; work=65536 kernel iterations per business call, one worker. Background load, CPU frequency, filesystem scheduling and caches were uncontrolled. No project builds, tests, allocation probes or analysis overlapped ordinary timing.

## Correctness and reproduction

The workload integration tests exercise both modes across all six cases and compare every row of all ten process/lease-counter tables after durable reopen-and-replay. An injected outcome-insert failure proves the distinct pending-input guarantees, zero partial outcomes/actions/heads/commit rows, and successful retry after reopen. Missing business-result evidence remains rejected. Executor all-target/all-feature checks pass on stable Rust and Rust 1.85.0, together with strict workspace Clippy, formatting and layer checks. Python tests validate known ratios, complete paired samples, admission isolation and negative controls.

Run `python3 scripts/benchmark-application-admission.py` from the repository root for the fixed default comparison. Run `python3 scripts/benchmark-application-workloads.py --admission atomic` for the complete six-path suite with atomic admission. See [workload documentation](application-workloads.md) for the contracts and limits.

[Raw evidence, hashes, qualification and verifier](verification-data/2026-10-31-application-admission/) retain all observations. The verifier recomputes the unrounded summary and checks qualification/source evidence against this version. Archived earlier workload reports remain pinned to their own versions; their source-hash verifiers require those checkouts.
