#!/usr/bin/env bash
set -euo pipefail

workflow_root="${1:-.github/workflows}"
status=0
while IFS= read -r line; do
  ref="${line##*@}"
  ref="${ref%%[[:space:]]*}"
  if [[ ! "$ref" =~ ^[0-9a-fA-F]{40}$ ]]; then
    printf 'mutable or malformed GitHub Action reference: %s\n' "$line" >&2
    status=1
  fi
done < <(rg --no-heading --line-number '^[[:space:]]*-[[:space:]]*uses:' "$workflow_root" || true)

if (( status != 0 )); then
  exit 1
fi
printf 'all GitHub Actions are pinned to 40-character commit SHAs\n'
