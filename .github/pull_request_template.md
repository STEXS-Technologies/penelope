## Description

Clear explanation of what this PR accomplishes and why it matters.
Include operator, protocol, or deployment context where relevant.

## Changes

Detailed list of what changed:

- File/module and what changed
- File/module and what changed
- Any crate-boundary, runtime, or deployment implications

For cross-crate or runtime changes, include:

- Affected crates
- Cross-crate interaction changes (if any) and why
- Migration or rollout requirements (if any)

## Motivation

Business or operator motivation:

Technical motivation:

Alternative approaches considered:

## Scope and impact

- Affected crates:
- Protocol or API changes:
- Backward compatibility:
- Performance impact:
- Security impact:
- Operational impact:

## Testing

- [ ] Unit tests
- [ ] Integration tests
- [ ] Manual verification
- [ ] Performance checks (if applicable)
- [ ] Security checks (if applicable)
- [ ] Recovery, fault-injection and fuzz checks (if applicable)
- [ ] Portable `no_std` core checks (if applicable)

Commands/results:

```bash
# Example:
cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo clippy --workspace --all-targets --all-features --locked --exclude penelope-fuzz -- -D warnings
```

## Related issues and documentation

- Fixes:
- Related:
- Architecture docs: `docs/RELEASE.md`
- Protocol docs: `docs/RELEASE.md#execution-contracts`
- Security/invariants docs: `SECURITY.md`
- Recovery and validation docs: `docs/RELEASE.md#validation`
- Operations/runbook updates:

## Reviewer checklist

- [ ] Code follows project standards and crate-boundary constraints
- [ ] Tests added or updated and passing
- [ ] Documentation updated where behavior or config changed
- [ ] No undocumented breaking change
- [ ] Performance trade-offs documented where relevant
- [ ] Security considerations addressed where relevant

## Additional notes

Trade-offs, follow-up work, or deviations from standards.
