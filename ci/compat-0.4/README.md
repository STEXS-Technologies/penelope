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

An import fixture does not establish behavior, exhaustive enum or wire parity.
Those require separate frozen examples and history/replay fixtures.

The original consumer source and declared package minimum remain frozen. The
0.5 dependency graph requires Rust 1.89 by an explicit compatibility decision;
this fixture does not promise that new dependencies build on Rust 1.85.
