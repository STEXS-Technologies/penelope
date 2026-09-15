# Changelog

All notable changes to Penelope are documented here.

## Unreleased

- Finalized the library-only release boundary and consumer-owned adapter
  responsibilities.
- Removed embedded DTO wire-version and schema-discriminator fields from the
  public protocol values.
- Kept `DefinitionVersion` as the semantic workflow-definition pin.
- Documented the deterministic saga/replay, outcome-log, retry, compensation,
  reconciliation, and typed-port contracts.
- Added the release checklist and reproducible verification commands.
- Maintained Rust 1.85 MSRV and stable CI compatibility.
