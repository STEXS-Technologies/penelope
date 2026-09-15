#!/usr/bin/env bash
set -euo pipefail

mode="dry-run"
if [[ "${1:-}" == "--publish" ]]; then
  mode="publish"
elif [[ "${1:-}" != "" ]]; then
  echo "usage: $0 [--publish]" >&2
  exit 2
fi

# Dependency-first order. The final facade depends on every public crate.
crates=(
  penelope-core
  penelope-domain
  penelope-intent
  penelope-ports
  penelope-executor
  penelope-statechronicle
  penelope
)

if [[ "$mode" == "dry-run" ]]; then
  # A first release cannot resolve dependent crates from crates.io until the
  # preceding crates exist. Package the complete local workspace first.
  cargo package --workspace --locked --allow-dirty --no-verify
else
  for crate in "${crates[@]}"; do
    cargo publish --locked -p "$crate"
    sleep 15
  done
fi
