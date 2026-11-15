# Frozen 0.4 consumer

This independent workspace checks selected released imports against the local
Penelope implementation, with and without the optional timeline feature.
It intentionally retains its own lockfile. Do not update its source just to
make incompatible 0.5 exports pass; record a deliberate migration decision.

From the repository root:

```sh
cargo check --manifest-path ci/compat-0.4/Cargo.toml --locked
cargo check --manifest-path ci/compat-0.4/Cargo.toml --all-features --locked
cargo +1.89.0 check --manifest-path ci/compat-0.4/Cargo.toml --all-features --locked
```

`src/lib.rs` remains the unchanged import fixture. `src/protocols.rs` adds selected
function-pointer signatures, exhaustive public enum matches, task event JSON and
history replay under every recovery/effect policy, and timeline metadata parsing
and checkpoint restoration. Its 33,278 expected observations were generated from
the unmodified 0.4 library checkout, with an independently retained lockfile and
source/output hashes in `baseline/manifest.json`.

Run `./scripts/check-saga-machine-parity.sh` from the repository root to compare
both saga paths and this task/timeline corpus. CI runs the gate on current Rust
and Rust 1.89. Keep expected bytes independent of candidate libraries. This is
bounded evidence, not exhaustive public API or SQLite migration qualification.

The original consumer source and declared package minimum remain frozen. The
0.5 dependency graph requires Rust 1.89 by an explicit compatibility decision;
this fixture does not promise that new dependencies build on Rust 1.85.
