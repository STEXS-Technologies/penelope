#!/usr/bin/env bash
set -euo pipefail

workflow_root="${1:-.github/workflows}"
status=0
if matches="$(grep -R -n -E '^[[:space:]]*-[[:space:]]*uses:' "$workflow_root")"; then
  :
else
  scan_status=$?
  if (( scan_status > 1 )); then
    printf 'failed to scan GitHub Actions in %s\n' "$workflow_root" >&2
    exit "$scan_status"
  fi
fi
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  ref="${line##*@}"
  ref="${ref%%[[:space:]]*}"
  if [[ ! "$ref" =~ ^[0-9a-fA-F]{40}$ ]]; then
    printf 'mutable or malformed GitHub Action reference: %s\n' "$line" >&2
    status=1
  fi
done <<< "$matches"

if (( status != 0 )); then
  exit 1
fi
printf 'all GitHub Actions are pinned to 40-character commit SHAs\n'
