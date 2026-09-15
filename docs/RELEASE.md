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

Publish dependency-first with the checked-in script. Its default mode packages
every tarball locally (a first release cannot resolve dependent crates from
crates.io before they are published) and uploads only with an explicit flag:

```bash
./scripts/publish-crates.sh
./scripts/publish-crates.sh --publish
```

The order is `penelope-core`, `penelope-domain`, `penelope-intent`,
`penelope-ports`, `penelope-executor`, `penelope-statechronicle`, and finally
the umbrella `penelope` crate. Wait for each crate to become available on
crates.io before the next dependent publish. If a publish is interrupted,
verify the exact version on crates.io and resume at the first unpublished
crate; already published crates cannot be replaced.

## Credentials and ownership

The release workflow requires a `CARGO_REGISTRY_TOKEN` repository secret only
when publishing is explicitly requested. Keep publishing credentials outside
the repository and use a crates.io account that owns all seven package names.
