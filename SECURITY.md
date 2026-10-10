# Security policy

## Supported versions

Penelope is a pre-1.0 library. This branch prepares the `0.2.0` release while
preserving the `0.1.0` saga protocol and behavior. The minimum
supported Rust version is 1.85 (Rust 2024 edition); CI enforces this
compatibility contract for the library workspace.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability involving tenant
isolation, duplicate effects, forged canonical events, secrets, authorization,
or data loss. Report it privately to the maintainers through the repository's
security-advisory reporting channel, including:

- affected revision and build command;
- a minimal reproduction or proof of concept;
- impact, tenant/process scope, and prerequisites; and
- any mitigation or disclosure deadline you propose.

Maintainers will acknowledge a report within seven calendar days, assess the
impact, coordinate a fix, and publish an advisory after users have a practical
upgrade or mitigation path. Please do not exploit a report beyond what is
necessary to demonstrate impact, access unrelated data, or disrupt services.

## Security boundaries

Penelope coordinates process state; it is not a ledger or authorization
system. Deployments must enforce tenant isolation, authorize every command,
store outcomes durably, and verify StateChronicle committed-event evidence
before advancing a saga. An unknown external result must be reconciled or
escalated, never blindly retried.

Task execution trusts journal and runtime adapters to supply authentic,
durable records, bounded polling, generation fencing, CPU enforcement and
independent reconciliation evidence. SHA-256 references identify bytes; they
do not authenticate a journal or prove application correctness. The portable
core checks lifecycle transitions and quotas without providing isolation or
preemption. See [the execution contracts](docs/RELEASE.md#execution-contracts)
for tested threats and explicit deployment responsibilities.
