# Penelope 0.1.0 release

Penelope publishes seven library crates at version `0.1.0`. The fuzz crate is
test-only and is never published. Internal dependencies include both a path
and a `0.1.0` version requirement so Cargo can resolve the crates after they
are uploaded.

## Before tagging

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features --locked --exclude penelope-fuzz
cargo clippy --workspace --all-targets --all-features --locked --exclude penelope-fuzz -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features --locked --exclude penelope-fuzz
cargo audit
cargo deny check
./scripts/check-action-pins.sh
./scripts/check-layer-boundaries.sh
./scripts/check-no-secrets.sh
./scripts/publish-crates.sh
```

Confirm the working tree is clean, review `CHANGELOG.md`, and create the tag:

```bash
git tag -a v0.1.0 -m "Penelope 0.1.0"
git push origin v0.1.0
```

## Publish order

Publish dependency-first with the checked-in script. It derives the exact order
from Cargo metadata, including normal, build, and dev path dependencies, rather
than maintaining a hand-written list. Its default mode packages every tarball
locally (a first release cannot resolve dependent crates from crates.io before
they are published) and uploads only with an explicit flag:

```bash
./scripts/publish-crates.sh
./scripts/publish-crates.sh --publish
```

The script waits for each exact version to enter the crates.io sparse index
before publishing its dependents, skips versions already published, and verifies
every expected crate/version after completing. It is therefore safe to rerun
after an interrupted release.

## Credentials and ownership

The release workflow requires a `CARGO_REGISTRY_TOKEN` repository secret only
when publishing is explicitly requested. Keep publishing credentials outside
the repository and use a crates.io account that owns all seven package names.
