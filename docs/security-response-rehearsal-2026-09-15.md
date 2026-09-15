# Security response rehearsal evidence — 2026-09-15

## Scope

Repository: Penelope library workspace  
Revision: `1edf859208a76fdda944b687c93484abf382c620`  
Purpose: verify that a dependency/security alert can be triaged without
cross-tenant disclosure, stale acknowledgements, or blind replay of effects.

## Executed controls

| Control | Command | Result |
| --- | --- | --- |
| RustSec advisory scan | `cargo audit --no-yanked` | Pass; 72 dependencies, no known vulnerabilities |
| License/source/ban policy | `cargo deny check advisories bans licenses sources` | Pass |
| Dependency layer boundary | `scripts/check-layer-boundaries.sh` | Pass |
| Reachable-history secret scan | `scripts/check-no-secrets.sh` | Pass; 0 leaks, 221 commits inspected |
| Locked library tests | `cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz` | Pass |
| Strict lint | `cargo clippy --workspace --all-targets --all-features --locked --exclude penelope-fuzz -- -D warnings` | Pass |
| Formatting | `cargo fmt --all --check` | Pass |

## Safety assertions exercised by the suite

- Unknown canonical/external outcomes produce escalation, never automatic
  retry.
- Pre-horizon `NotCommitted` evidence produces a wait disposition; only
  post-horizon authoritative absence produces retry.
- Cross-tenant/process/action/definition substitutions are rejected by typed
  validators and receipt checks.
- Expired timer and outbox leases cannot be acknowledged.
- Atomic commit fault injection exposes no partial input, outcome, or action.
- Redacted diagnostics contain no raw payload or error message field.

## Decision

The library gate passes for this revision. No production adapter, credentials,
broker, database, or external canonical system was contacted by this rehearsal.
Adapter-specific incident drills and release promotion remain composition-root
responsibilities.
