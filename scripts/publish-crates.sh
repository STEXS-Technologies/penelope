#!/usr/bin/env bash
set -euo pipefail

mode="dry-run"
if [[ "${1:-}" == "--publish" ]]; then
  mode="publish"
elif [[ "${1:-}" != "" ]]; then
  echo "usage: $0 [--publish]" >&2
  exit 2
fi

mapfile -t crates < <(python3 scripts/publish-order.py --emit)

if [[ "${#crates[@]}" -eq 0 ]]; then
  echo "no publishable workspace crates found" >&2
  exit 1
fi

if [[ "$mode" == "dry-run" ]]; then
  # A first release cannot resolve dependent crates from crates.io until the
  # preceding crates exist. Package the complete local workspace first.
  cargo package --workspace --locked --allow-dirty --no-verify
else
  if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
    echo "CARGO_REGISTRY_TOKEN is required to publish crates" >&2
    exit 1
  fi

  crate_version() {
    cargo pkgid -p "$1" | sed -E 's/.*#//; s/.*@//'
  }

  crate_version_published() {
    curl --fail --silent --show-error --retry 3 --retry-delay 3 \
      -H "User-Agent: penelope-release-ci (curl)" \
      "https://crates.io/api/v1/crates/$1/$2" >/dev/null 2>&1
  }

  for crate in "${crates[@]}"; do
    version="$(crate_version "$crate")"
    if crate_version_published "$crate" "$version"; then
      echo "$crate $version already exists on crates.io; skipping"
      continue
    fi
    if ! cargo publish --locked -p "$crate"; then
      if crate_version_published "$crate" "$version"; then
        echo "$crate $version was published concurrently; continuing"
        continue
      fi
      echo "failed to publish $crate $version" >&2
      exit 1
    fi
    python3 scripts/publish-order.py --wait "$crate" "$version"
  done

  for crate in "${crates[@]}"; do
    version="$(crate_version "$crate")"
    if ! crate_version_published "$crate" "$version"; then
      echo "published crate missing from crates.io: $crate $version" >&2
      exit 1
    fi
  done
fi
